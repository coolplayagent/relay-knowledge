//! Repository-set overlay refresh, edge resolution, and status projection.

use rusqlite::{Connection, OptionalExtension, Row, Transaction, TransactionBehavior, params};
use serde_json::json;

use crate::{
    clock::system_now_millis as shared_system_now_millis,
    domain::{
        CodeRepositoryCrossEdge, CodeRepositorySetMember, CodeRepositorySetMemberStatus,
        CodeRepositorySetOverlayStatus, CodeRepositorySetRefreshSummary, CodeRepositorySetStatus,
    },
    storage::{CodeRepositorySetMemberSeed, CodeRepositorySetRefreshPublication, StorageError},
};

use super::super::super::evidence_identity::stable_id;
use super::{
    capacity::{
        MAX_REPOSITORY_SET_MEMBERS, MAX_REPOSITORY_SET_OVERLAY_EDGES,
        MAX_REPOSITORY_SET_OVERLAY_IMPORT_SCAN_ROWS, REPOSITORY_SET_OVERLAY_IMPORT_PAGE_ROWS,
        capacity_error, ensure_overlay_delete_is_bounded,
    },
    manifest::normalize_module_key,
    membership::{member_statuses, set_by_alias},
};

mod export_index;
mod projection;

use export_index::{ExportIndex, ExportTarget};
pub(in crate::storage::sqlite::code) use projection::cross_edges_for_selector;

const IMPORT_PAGE_FIRST_SQL: &str = "
    SELECT repository_id, source_scope, import_id, path, module, target_hint,
           resolution_state, line_start, line_end
    FROM code_repository_imports
    WHERE source_scope = ?1
    ORDER BY import_id ASC
    LIMIT ?2
";

const IMPORT_PAGE_AFTER_SQL: &str = "
    SELECT repository_id, source_scope, import_id, path, module, target_hint,
           resolution_state, line_start, line_end
    FROM code_repository_imports
    WHERE source_scope = ?1 AND import_id > ?2
    ORDER BY import_id ASC
    LIMIT ?3
";

pub(in super::super) fn set_status(
    connection: &mut Connection,
    alias: &str,
) -> Result<Option<CodeRepositorySetStatus>, StorageError> {
    let Some(set) = set_by_alias(connection, alias)? else {
        return Ok(None);
    };
    let members = member_statuses(connection, &set.set_id)?;
    let overlay = overlay_status(connection, &set.set_id, &members)?;
    let member_stale = members.iter().any(|member| member.stale);
    let freshness_state = if members.is_empty() {
        "incomplete"
    } else if member_stale {
        "stale"
    } else if overlay.stale {
        "overlay_stale"
    } else {
        "fresh"
    }
    .to_owned();
    let degraded_reason = members
        .iter()
        .find_map(|member| member.degraded_reason.clone())
        .or_else(|| overlay.degraded_reason.clone());

    Ok(Some(CodeRepositorySetStatus {
        repository_set: set,
        members,
        overlay,
        freshness_state,
        degraded_reason,
    }))
}

pub(in super::super) fn refresh_overlay_for_task(
    connection: &mut Connection,
    alias: &str,
    publication: CodeRepositorySetRefreshPublication,
) -> Result<CodeRepositorySetRefreshSummary, StorageError> {
    refresh_overlay_with_publication(connection, alias, None, Some(publication))
}

#[cfg(test)]
pub(in super::super) fn refresh_overlay(
    connection: &mut Connection,
    alias: &str,
    now_ms: u64,
) -> Result<CodeRepositorySetRefreshSummary, StorageError> {
    refresh_overlay_with_publication(connection, alias, Some(now_ms), None)
}

fn refresh_overlay_with_publication(
    connection: &mut Connection,
    alias: &str,
    requested_now_ms: Option<u64>,
    publication: Option<CodeRepositorySetRefreshPublication>,
) -> Result<CodeRepositorySetRefreshSummary, StorageError> {
    let status = set_status(connection, alias)?.ok_or_else(|| {
        StorageError::InvalidInput(format!("code repository set '{alias}' is not registered"))
    })?;
    if status.members.is_empty() {
        return Err(StorageError::InvalidInput(format!(
            "code repository set '{}' has no members",
            status.repository_set.alias
        )));
    }

    let initial_member_versions = member_versions_json(&status.members)?;
    let status = publication.as_ref().map_or_else(
        || Ok(status.clone()),
        |publication| status_with_replacements(connection, status.clone(), publication),
    )?;
    let exports = ExportIndex::for_members(connection, &status.members)?;
    let mut edges = candidate_edges_for_members(
        connection,
        &status.repository_set.set_id,
        &status.members,
        &exports,
        MAX_REPOSITORY_SET_OVERLAY_IMPORT_SCAN_ROWS,
    )?;

    let now_ms = requested_now_ms.map_or_else(system_now_millis, Ok)?;
    for edge in &mut edges {
        edge.created_at_ms = now_ms;
    }
    let expected_member_versions = member_versions_json(&status.members)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(publication) = publication.as_ref() {
        validate_refresh_publication(&transaction, alias, publication, now_ms)?;
        if persisted_member_versions_json(&transaction, &publication.set_id)?
            != initial_member_versions
        {
            return Err(StorageError::InvalidInput(format!(
                "code repository set '{alias}' changed while its overlay was being built"
            )));
        }
        for replacement in &publication.member_replacements {
            publish_member_replacement(
                &transaction,
                &publication.set_id,
                alias,
                replacement,
                now_ms,
            )?;
        }
        if persisted_member_versions_json(&transaction, &publication.set_id)?
            != expected_member_versions
        {
            return Err(StorageError::InvalidInput(format!(
                "code repository set '{alias}' replacement scopes changed before publication"
            )));
        }
    }
    ensure_overlay_delete_is_bounded(&transaction, &status.repository_set.set_id)?;
    transaction.execute(
        "DELETE FROM code_repository_cross_edges WHERE set_id = ?1",
        params![status.repository_set.set_id],
    )?;
    for edge in &edges {
        transaction.execute(
            "
            INSERT INTO code_repository_cross_edges (
                edge_id, set_id, from_source_scope, from_repository_id, from_record_kind,
                from_record_id, to_source_scope, to_repository_id, to_record_kind, to_record_id,
                edge_kind, resolution_state, confidence_basis_points, confidence_tier,
                evidence_json, created_at_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ",
            params![
                edge.edge_id,
                edge.set_id,
                edge.from_source_scope,
                edge.from_repository_id,
                edge.from_record_kind,
                edge.from_record_id,
                edge.to_source_scope,
                edge.to_repository_id,
                edge.to_record_kind,
                edge.to_record_id,
                edge.edge_kind,
                edge.resolution_state,
                edge.confidence_basis_points,
                edge.confidence_tier,
                edge.evidence_json,
                edge.created_at_ms,
            ],
        )?;
    }
    transaction.execute(
        "
        INSERT INTO code_repository_set_overlay_status (
            set_id, state, refreshed_at_ms, edge_count, member_versions_json, degraded_reason
        )
        VALUES (?1, 'fresh', ?2, ?3, ?4, NULL)
        ON CONFLICT(set_id) DO UPDATE SET
            state = excluded.state,
            refreshed_at_ms = excluded.refreshed_at_ms,
            edge_count = excluded.edge_count,
            member_versions_json = excluded.member_versions_json,
            degraded_reason = NULL
        ",
        params![
            status.repository_set.set_id,
            now_ms,
            edges.len(),
            expected_member_versions,
        ],
    )?;
    transaction.commit()?;

    Ok(CodeRepositorySetRefreshSummary {
        set_id: status.repository_set.set_id,
        alias: status.repository_set.alias,
        edge_count: edges.len(),
        resolved_edge_count: edges
            .iter()
            .filter(|edge| edge.resolution_state == "resolved")
            .count(),
        ambiguous_edge_count: edges
            .iter()
            .filter(|edge| edge.resolution_state == "ambiguous")
            .count(),
        unresolved_edge_count: edges
            .iter()
            .filter(|edge| edge.resolution_state == "unresolved")
            .count(),
        refreshed_at_ms: now_ms,
    })
}

fn status_with_replacements(
    connection: &Connection,
    mut status: CodeRepositorySetStatus,
    publication: &CodeRepositorySetRefreshPublication,
) -> Result<CodeRepositorySetStatus, StorageError> {
    if publication.member_replacements.len() > MAX_REPOSITORY_SET_MEMBERS {
        return Err(capacity_error(
            "member replacement",
            MAX_REPOSITORY_SET_MEMBERS,
        ));
    }
    for (index, replacement) in publication.member_replacements.iter().enumerate() {
        validate_replacement_identity(
            &status,
            &publication.member_replacements[..index],
            replacement,
        )?;
        let replacement_status =
            replacement_member_status(connection, &publication.set_id, replacement)?;
        status
            .members
            .retain(|member| member.member.repository_id != replacement.repository_id);
        status.members.push(replacement_status);
    }
    status.members.sort_by(|left, right| {
        right
            .member
            .priority
            .cmp(&left.member.priority)
            .then_with(|| {
                left.member
                    .repository_alias
                    .cmp(&right.member.repository_alias)
            })
            .then_with(|| left.member.source_scope.cmp(&right.member.source_scope))
    });
    Ok(status)
}

fn validate_replacement_identity(
    status: &CodeRepositorySetStatus,
    earlier: &[CodeRepositorySetMemberSeed],
    replacement: &CodeRepositorySetMemberSeed,
) -> Result<(), StorageError> {
    if replacement.set_alias != status.repository_set.alias {
        return Err(StorageError::InvalidInput(format!(
            "repository set member replacement targets alias '{}', expected '{}'",
            replacement.set_alias, status.repository_set.alias
        )));
    }
    if earlier
        .iter()
        .any(|candidate| candidate.repository_id == replacement.repository_id)
    {
        return Err(StorageError::InvalidInput(format!(
            "repository '{}' has duplicate repository-set member replacements",
            replacement.repository_id
        )));
    }
    if !status
        .members
        .iter()
        .any(|member| member.member.repository_id == replacement.repository_id)
    {
        return Err(StorageError::InvalidInput(format!(
            "repository '{}' is not a member of repository set '{}'",
            replacement.repository_id, status.repository_set.alias
        )));
    }
    Ok(())
}

fn replacement_member_status(
    connection: &Connection,
    set_id: &str,
    replacement: &CodeRepositorySetMemberSeed,
) -> Result<CodeRepositorySetMemberStatus, StorageError> {
    let scope = connection
        .query_row(
            "
            SELECT tree_hash, stale, indexed_file_count, symbol_count, reference_count,
                   chunk_count, degraded_reason, path_filters_json, language_filters_json
            FROM code_repository_scopes
            WHERE source_scope = ?1 AND repository_id = ?2 AND retiring = 0
            ",
            params![replacement.source_scope, replacement.repository_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, usize>(2)?,
                    row.get::<_, usize>(3)?,
                    row.get::<_, usize>(4)?,
                    row.get::<_, usize>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "repository set member replacement scope '{}' is not a live indexed scope for repository '{}'",
                replacement.source_scope, replacement.repository_id
            ))
        })?;
    let indexed_path_filters = serde_json::from_str(&scope.7)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let indexed_language_filters = serde_json::from_str(&scope.8)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    Ok(CodeRepositorySetMemberStatus {
        member: CodeRepositorySetMember {
            set_id: set_id.to_owned(),
            repository_id: replacement.repository_id.clone(),
            repository_alias: replacement.repository_alias.clone(),
            ref_selector: replacement.ref_selector.clone(),
            resolved_commit_sha: replacement.resolved_commit_sha.clone(),
            source_scope: replacement.source_scope.clone(),
            path_filters: replacement.path_filters.clone(),
            language_filters: replacement.language_filters.clone(),
            priority: replacement.priority,
        },
        tree_hash: scope.0,
        indexed_path_filters,
        indexed_language_filters,
        freshness_state: if scope.1 { "stale" } else { "fresh" }.to_owned(),
        stale: scope.1,
        indexed_file_count: scope.2,
        symbol_count: scope.3,
        reference_count: scope.4,
        chunk_count: scope.5,
        degraded_reason: scope.6,
    })
}

fn publish_member_replacement(
    transaction: &Transaction<'_>,
    set_id: &str,
    alias: &str,
    replacement: &CodeRepositorySetMemberSeed,
    now_ms: u64,
) -> Result<(), StorageError> {
    let valid_target = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM code_repository_scopes
             WHERE source_scope = ?1 AND repository_id = ?2 AND retiring = 0
         )",
        params![replacement.source_scope, replacement.repository_id],
        |row| row.get::<_, bool>(0),
    )?;
    if replacement.set_alias != alias || !valid_target {
        return Err(StorageError::InvalidInput(format!(
            "repository set member replacement for '{}' is no longer valid",
            replacement.repository_alias
        )));
    }
    let removed = transaction.execute(
        "DELETE FROM code_repository_set_members WHERE set_id = ?1 AND repository_id = ?2",
        params![set_id, replacement.repository_id],
    )?;
    if removed != 1 {
        return Err(StorageError::InvalidInput(format!(
            "repository '{}' is no longer a member of repository set '{alias}'",
            replacement.repository_id
        )));
    }
    transaction.execute(
        "
        INSERT INTO code_repository_set_members (
            set_id, repository_id, repository_alias, ref_selector, resolved_commit_sha,
            source_scope, path_filters_json, language_filters_json, priority
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
        params![
            set_id,
            replacement.repository_id,
            replacement.repository_alias,
            replacement.ref_selector,
            replacement.resolved_commit_sha,
            replacement.source_scope,
            serde_json::to_string(&replacement.path_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            serde_json::to_string(&replacement.language_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            replacement.priority,
        ],
    )?;
    transaction.execute(
        "UPDATE code_repository_sets SET updated_at_ms = ?2 WHERE set_id = ?1",
        params![set_id, now_ms],
    )?;
    Ok(())
}

fn persisted_member_versions_json(
    connection: &Connection,
    set_id: &str,
) -> Result<String, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT member.repository_id, member.source_scope, member.resolved_commit_sha,
               scope.tree_hash, scope.stale
        FROM code_repository_set_members member
        JOIN code_repository_scopes scope ON scope.source_scope = member.source_scope
        WHERE member.set_id = ?1 AND scope.retiring = 0
        ORDER BY member.priority DESC, member.repository_alias ASC, member.source_scope ASC
        LIMIT ?2
        ",
    )?;
    let versions = statement
        .query_map(params![set_id, MAX_REPOSITORY_SET_MEMBERS + 1], |row| {
            Ok(json!({
                "repository_id": row.get::<_, String>(0)?,
                "source_scope": row.get::<_, String>(1)?,
                "resolved_commit_sha": row.get::<_, String>(2)?,
                "tree_hash": row.get::<_, String>(3)?,
                "stale": row.get::<_, i64>(4)? != 0,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if versions.len() > MAX_REPOSITORY_SET_MEMBERS {
        return Err(capacity_error("member", MAX_REPOSITORY_SET_MEMBERS));
    }
    serde_json::to_string(&versions).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn validate_refresh_publication(
    transaction: &Transaction<'_>,
    alias: &str,
    publication: &CodeRepositorySetRefreshPublication,
    now_ms: u64,
) -> Result<(), StorageError> {
    let authorized = transaction.query_row(
        "
        SELECT EXISTS (
            SELECT 1
            FROM code_repository_set_refresh_tasks task
            JOIN code_repository_sets repository_set ON repository_set.set_id = task.set_id
            WHERE task.task_id = ?1
              AND task.set_id = ?2
              AND task.set_alias = ?3
              AND repository_set.alias = ?3
              AND task.state = 'running'
              AND task.lease_owner = ?4
              AND task.attempt_count = ?5
              AND task.lease_expires_at_ms > ?6
        )
        ",
        params![
            publication.task_id,
            publication.set_id,
            alias,
            publication.lease_owner,
            publication.attempt_count,
            now_ms,
        ],
        |row| row.get::<_, bool>(0),
    )?;
    if !authorized {
        return Err(StorageError::InvalidInput(
            "repository set refresh publication lease is no longer active".to_owned(),
        ));
    }
    Ok(())
}

fn system_now_millis() -> Result<u64, StorageError> {
    shared_system_now_millis()
        .map_err(|error| StorageError::InvalidInput(format!("system clock is invalid: {error}")))
}

pub(in super::super) fn cross_edges_for_set(
    connection: &mut Connection,
    set_id: &str,
) -> Result<Vec<CodeRepositoryCrossEdge>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT edge_id, set_id, from_source_scope, from_repository_id, from_record_kind,
               from_record_id, to_source_scope, to_repository_id, to_record_kind, to_record_id,
               edge_kind, resolution_state, confidence_basis_points, confidence_tier,
               evidence_json, created_at_ms
        FROM code_repository_cross_edges edge
        WHERE edge.set_id = ?1
          AND EXISTS (
              SELECT 1
              FROM code_repository_set_members member
              WHERE member.set_id = edge.set_id
                AND member.source_scope = edge.from_source_scope
          )
          AND EXISTS (
              SELECT 1 FROM code_repository_scopes source_scope
              WHERE source_scope.source_scope = edge.from_source_scope
                AND source_scope.retiring = 0
          )
          AND (
              edge.to_source_scope IS NULL OR EXISTS (
                  SELECT 1 FROM code_repository_scopes target_scope
                  WHERE target_scope.source_scope = edge.to_source_scope
                    AND target_scope.retiring = 0
              )
          )
        ORDER BY from_source_scope ASC, from_record_id ASC, edge_id ASC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(
        params![set_id, MAX_REPOSITORY_SET_OVERLAY_EDGES + 1],
        edge_from_row,
    )?;
    let edges = rows.collect::<Result<Vec<_>, _>>()?;
    if edges.len() > MAX_REPOSITORY_SET_OVERLAY_EDGES {
        return Err(capacity_error(
            "edge read",
            MAX_REPOSITORY_SET_OVERLAY_EDGES,
        ));
    }
    Ok(edges)
}

fn overlay_status(
    connection: &mut Connection,
    set_id: &str,
    members: &[CodeRepositorySetMemberStatus],
) -> Result<CodeRepositorySetOverlayStatus, StorageError> {
    let current_versions = member_versions_json(members)?;
    let stored = connection
        .query_row(
            "
            SELECT state, refreshed_at_ms, edge_count, member_versions_json, degraded_reason
            FROM code_repository_set_overlay_status
            WHERE set_id = ?1
            ",
            params![set_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<u64>>(1)?,
                    row.get::<_, usize>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((state, refreshed_at_ms, edge_count, member_versions, degraded_reason)) = stored
    else {
        return Ok(CodeRepositorySetOverlayStatus {
            state: "missing".to_owned(),
            stale: true,
            edge_count: 0,
            refreshed_at_ms: None,
            degraded_reason: None,
        });
    };
    if edge_count > MAX_REPOSITORY_SET_OVERLAY_EDGES {
        return Err(capacity_error(
            "stored edge",
            MAX_REPOSITORY_SET_OVERLAY_EDGES,
        ));
    }
    let stale = member_versions != current_versions;

    Ok(CodeRepositorySetOverlayStatus {
        state: if stale {
            "overlay_stale".to_owned()
        } else {
            state
        },
        stale,
        edge_count,
        refreshed_at_ms,
        degraded_reason,
    })
}

fn candidate_edges_for_members(
    connection: &mut Connection,
    set_id: &str,
    members: &[CodeRepositorySetMemberStatus],
    exports: &ExportIndex,
    max_import_scan_rows: usize,
) -> Result<Vec<CodeRepositoryCrossEdge>, StorageError> {
    let mut scanned = 0usize;
    let mut edges = Vec::new();
    for member in members {
        let mut cursor = None;
        loop {
            let remaining = max_import_scan_rows.saturating_sub(scanned);
            let page_capacity = remaining.min(REPOSITORY_SET_OVERLAY_IMPORT_PAGE_ROWS);
            let mut page = import_page(
                connection,
                &member.member.source_scope,
                cursor.as_ref(),
                page_capacity.saturating_add(1),
            )?;
            if page.len() > page_capacity {
                if page_capacity < REPOSITORY_SET_OVERLAY_IMPORT_PAGE_ROWS {
                    return Err(capacity_error("import scan row", max_import_scan_rows));
                }
                // The extra row is only a continuation sentinel. Leave it for
                // the next keyset page so each processed page stays bounded.
                page.pop();
            }
            if page.is_empty() {
                break;
            }
            scanned = advance_import_scan(scanned, page.len(), max_import_scan_rows)?;
            for import in &page {
                let Some(candidates) = matching_exports(import, exports) else {
                    continue;
                };
                // The source import remains the authoritative unresolved
                // metadata when no set member exports a candidate. The set
                // overlay stores only relationships introduced by membership.
                if candidates.is_empty() {
                    continue;
                }
                if edges.len() >= MAX_REPOSITORY_SET_OVERLAY_EDGES {
                    return Err(capacity_error("edge", MAX_REPOSITORY_SET_OVERLAY_EDGES));
                }
                edges.push(edge_for_import(set_id, import, &candidates, 0));
            }
            cursor = page.last().map(ImportCursor::from);
        }
    }

    Ok(edges)
}

fn advance_import_scan(
    scanned: usize,
    page_len: usize,
    max_import_scan_rows: usize,
) -> Result<usize, StorageError> {
    let next = scanned
        .checked_add(page_len)
        .ok_or_else(|| capacity_error("import scan row", max_import_scan_rows))?;
    if next > max_import_scan_rows {
        return Err(capacity_error("import scan row", max_import_scan_rows));
    }
    Ok(next)
}

fn import_page(
    connection: &Connection,
    source_scope: &str,
    cursor: Option<&ImportCursor>,
    limit: usize,
) -> Result<Vec<ImportRecord>, StorageError> {
    let limit = i64::try_from(limit).map_err(|_| {
        StorageError::CapacityExceeded(format!(
            "repository-set overlay import page limit {limit} exceeds SQLite integer capacity"
        ))
    })?;
    let (sql, values) = match cursor {
        Some(cursor) => (
            IMPORT_PAGE_AFTER_SQL,
            vec![
                rusqlite::types::Value::Text(source_scope.to_owned()),
                rusqlite::types::Value::Text(cursor.import_id.clone()),
                rusqlite::types::Value::Integer(limit),
            ],
        ),
        None => (
            IMPORT_PAGE_FIRST_SQL,
            vec![
                rusqlite::types::Value::Text(source_scope.to_owned()),
                rusqlite::types::Value::Integer(limit),
            ],
        ),
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(values), import_from_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn import_from_row(row: &Row<'_>) -> rusqlite::Result<ImportRecord> {
    Ok(ImportRecord {
        repository_id: row.get(0)?,
        source_scope: row.get(1)?,
        import_id: row.get(2)?,
        path: row.get(3)?,
        module: row.get(4)?,
        target_hint: row.get(5)?,
        resolution_state: row.get(6)?,
        line_start: row.get(7)?,
        line_end: row.get(8)?,
    })
}

fn matching_exports<'a>(
    import: &ImportRecord,
    exports: &'a ExportIndex,
) -> Option<Vec<&'a ExportTarget>> {
    if import.resolution_state != "unresolved" || is_local_or_relative_module(&import.module) {
        return None;
    }
    let module = normalize_module_key(&import.module);
    let mut candidates = exports.matching_targets(&import.source_scope, &module);
    candidates.sort_by(|left, right| {
        left.source_scope
            .cmp(&right.source_scope)
            .then_with(|| left.record_kind.cmp(&right.record_kind))
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    candidates.dedup_by(|left, right| {
        left.source_scope == right.source_scope
            && left.record_kind == right.record_kind
            && left.record_id == right.record_id
    });

    Some(candidates)
}

fn edge_for_import(
    set_id: &str,
    import: &ImportRecord,
    candidates: &[&ExportTarget],
    now_ms: u64,
) -> CodeRepositoryCrossEdge {
    let (state, confidence, tier, target) = match candidates {
        [target] => ("resolved", 10_000, "explicit", Some(target)),
        [] => ("unresolved", 0, "unresolved", None),
        _ => ("ambiguous", 5_000, "ambiguous", None),
    };
    let edge_id = stable_id(
        "code-repository-cross-edge",
        &format!(
            "{set_id}:{}:{}:{}:{state}",
            import.source_scope, import.import_id, import.module
        ),
    );
    let evidence_json = json!({
        "module": import.module,
        "target_hint": import.target_hint,
        "from_path": import.path,
        "from_line_start": import.line_start,
        "from_line_end": import.line_end,
        "candidate_count": candidates.len(),
        "candidate_record_ids": candidates.iter().take(10).map(|candidate| candidate.record_id.as_str()).collect::<Vec<_>>(),
    })
    .to_string();

    CodeRepositoryCrossEdge {
        edge_id,
        set_id: set_id.to_owned(),
        from_source_scope: import.source_scope.clone(),
        from_repository_id: import.repository_id.clone(),
        from_record_kind: "module_reference".to_owned(),
        from_record_id: import.import_id.clone(),
        to_source_scope: target.map(|target| target.source_scope.clone()),
        to_repository_id: target.map(|target| target.repository_id.clone()),
        to_record_kind: target
            .map(|target| target.record_kind.clone())
            .unwrap_or_else(|| "unresolved_target".to_owned()),
        to_record_id: target.map(|target| target.record_id.clone()),
        edge_kind: "imports".to_owned(),
        resolution_state: state.to_owned(),
        confidence_basis_points: confidence,
        confidence_tier: tier.to_owned(),
        evidence_json,
        created_at_ms: now_ms,
    }
}

fn member_versions_json(members: &[CodeRepositorySetMemberStatus]) -> Result<String, StorageError> {
    let versions = members
        .iter()
        .map(|member| {
            json!({
                "repository_id": member.member.repository_id,
                "source_scope": member.member.source_scope,
                "resolved_commit_sha": member.member.resolved_commit_sha,
                "tree_hash": member.tree_hash,
                "stale": member.stale,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&versions).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

pub(super) fn edge_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositoryCrossEdge> {
    Ok(CodeRepositoryCrossEdge {
        edge_id: row.get(0)?,
        set_id: row.get(1)?,
        from_source_scope: row.get(2)?,
        from_repository_id: row.get(3)?,
        from_record_kind: row.get(4)?,
        from_record_id: row.get(5)?,
        to_source_scope: row.get(6)?,
        to_repository_id: row.get(7)?,
        to_record_kind: row.get(8)?,
        to_record_id: row.get(9)?,
        edge_kind: row.get(10)?,
        resolution_state: row.get(11)?,
        confidence_basis_points: row.get(12)?,
        confidence_tier: row.get(13)?,
        evidence_json: row.get(14)?,
        created_at_ms: row.get(15)?,
    })
}

#[derive(Debug, Clone)]
struct ImportRecord {
    repository_id: String,
    source_scope: String,
    import_id: String,
    path: String,
    module: String,
    target_hint: Option<String>,
    resolution_state: String,
    line_start: u32,
    line_end: u32,
}

struct ImportCursor {
    import_id: String,
}

impl From<&ImportRecord> for ImportCursor {
    fn from(import: &ImportRecord) -> Self {
        Self {
            import_id: import.import_id.clone(),
        }
    }
}

fn is_local_or_relative_module(module: &str) -> bool {
    let module = module.trim();
    module.starts_with("./")
        || module.starts_with("../")
        || module.starts_with('.')
        || module.starts_with("crate::")
        || module.starts_with("self::")
        || module.starts_with("super::")
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod overlay_tests;
