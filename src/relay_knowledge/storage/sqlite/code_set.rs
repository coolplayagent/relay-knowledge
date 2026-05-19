use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::json;

use crate::{
    domain::{
        CodeRepositoryCrossEdge, CodeRepositorySet, CodeRepositorySetMember,
        CodeRepositorySetMemberStatus, CodeRepositorySetOverlayStatus,
        CodeRepositorySetRefreshSummary, CodeRepositorySetStatus,
    },
    storage::{CodeRepositorySetMemberSeed, CodeRepositorySetSeed, StorageError},
};

use super::{super::helpers::stable_id, code_status::parse_json_list};

pub(super) fn create_set(
    connection: &mut Connection,
    seed: CodeRepositorySetSeed,
) -> Result<CodeRepositorySet, StorageError> {
    let set_id = stable_id("code-repository-set", &seed.alias);
    connection.execute(
        "
        INSERT INTO code_repository_sets (
            set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ON CONFLICT(alias) DO UPDATE SET
            description = excluded.description,
            default_ref_policy_json = excluded.default_ref_policy_json,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![
            set_id,
            seed.alias,
            seed.description,
            seed.default_ref_policy_json,
            seed.now_ms,
        ],
    )?;

    set_by_alias(connection, &seed.alias)?.ok_or_else(|| {
        StorageError::InvalidInput("code repository set was not persisted".to_owned())
    })
}

pub(super) fn add_member(
    connection: &mut Connection,
    seed: CodeRepositorySetMemberSeed,
) -> Result<CodeRepositorySetMember, StorageError> {
    let set = set_by_alias(connection, &seed.set_alias)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository set '{}' is not registered",
            seed.set_alias
        ))
    })?;
    let scope_repository_id = connection
        .query_row(
            "
            SELECT repository_id
            FROM code_repository_scopes
            WHERE source_scope = ?1
            ",
            params![seed.source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "repository set member scope '{}' is not indexed",
                seed.source_scope
            ))
        })?;
    if scope_repository_id != seed.repository_id {
        return Err(StorageError::InvalidInput(format!(
            "repository set member scope '{}' belongs to repository '{}', not '{}'",
            seed.source_scope, scope_repository_id, seed.repository_id
        )));
    }

    connection.execute(
        "
        INSERT INTO code_repository_set_members (
            set_id, repository_id, repository_alias, ref_selector, resolved_commit_sha,
            source_scope, path_filters_json, language_filters_json, priority
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(set_id, repository_id, source_scope) DO UPDATE SET
            repository_alias = excluded.repository_alias,
            ref_selector = excluded.ref_selector,
            resolved_commit_sha = excluded.resolved_commit_sha,
            path_filters_json = excluded.path_filters_json,
            language_filters_json = excluded.language_filters_json,
            priority = excluded.priority
        ",
        params![
            set.set_id,
            seed.repository_id,
            seed.repository_alias,
            seed.ref_selector,
            seed.resolved_commit_sha,
            seed.source_scope,
            json_list(&seed.path_filters)?,
            json_list(&seed.language_filters)?,
            seed.priority,
        ],
    )?;
    connection.execute(
        "
        UPDATE code_repository_sets
        SET updated_at_ms = strftime('%s','now') * 1000
        WHERE set_id = ?1
        ",
        params![set.set_id],
    )?;

    member_by_key(
        connection,
        &set.set_id,
        &seed.repository_id,
        &seed.source_scope,
    )?
    .ok_or_else(|| StorageError::InvalidInput("repository set member was not persisted".to_owned()))
}

pub(super) fn set_by_alias(
    connection: &mut Connection,
    alias: &str,
) -> Result<Option<CodeRepositorySet>, StorageError> {
    connection
        .query_row(
            "
            SELECT set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms
            FROM code_repository_sets
            WHERE alias = ?1 OR set_id = ?1
            ",
            params![alias],
            set_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn set_status(
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

pub(super) fn refresh_overlay(
    connection: &mut Connection,
    alias: &str,
    now_ms: u64,
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

    let imports = imports_for_members(connection, &status.members)?;
    let exports = exports_for_members(connection, &status.members)?;
    let mut edges = Vec::new();
    for import in imports {
        let candidates = matching_exports(&import, &exports);
        edges.push(edge_for_import(
            &status.repository_set.set_id,
            &import,
            &candidates,
            now_ms,
        ));
    }

    let transaction = connection.transaction()?;
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
            member_versions_json(&status.members)?,
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

pub(super) fn cross_edges_for_set(
    connection: &mut Connection,
    set_id: &str,
) -> Result<Vec<CodeRepositoryCrossEdge>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT edge_id, set_id, from_source_scope, from_repository_id, from_record_kind,
               from_record_id, to_source_scope, to_repository_id, to_record_kind, to_record_id,
               edge_kind, resolution_state, confidence_basis_points, confidence_tier,
               evidence_json, created_at_ms
        FROM code_repository_cross_edges
        WHERE set_id = ?1
        ORDER BY from_source_scope ASC, from_record_id ASC, edge_id ASC
        ",
    )?;
    let rows = statement.query_map(params![set_id], edge_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn member_by_key(
    connection: &mut Connection,
    set_id: &str,
    repository_id: &str,
    source_scope: &str,
) -> Result<Option<CodeRepositorySetMember>, StorageError> {
    connection
        .query_row(
            "
            SELECT set_id, repository_id, repository_alias, ref_selector, resolved_commit_sha,
                   source_scope, path_filters_json, language_filters_json, priority
            FROM code_repository_set_members
            WHERE set_id = ?1 AND repository_id = ?2 AND source_scope = ?3
            ",
            params![set_id, repository_id, source_scope],
            member_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn member_statuses(
    connection: &mut Connection,
    set_id: &str,
) -> Result<Vec<CodeRepositorySetMemberStatus>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT member.set_id, member.repository_id, member.repository_alias, member.ref_selector,
               member.resolved_commit_sha, member.source_scope, member.path_filters_json,
               member.language_filters_json, member.priority, scope.tree_hash, scope.stale,
               scope.indexed_file_count, scope.symbol_count, scope.reference_count,
               scope.chunk_count, scope.degraded_reason
        FROM code_repository_set_members member
        JOIN code_repository_scopes scope ON scope.source_scope = member.source_scope
        WHERE member.set_id = ?1
        ORDER BY member.priority DESC, member.repository_alias ASC, member.source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![set_id], member_status_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
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

fn imports_for_members(
    connection: &mut Connection,
    members: &[CodeRepositorySetMemberStatus],
) -> Result<Vec<ImportRecord>, StorageError> {
    let mut imports = Vec::new();
    for member in members {
        let mut statement = connection.prepare(
            "
            SELECT repository_id, source_scope, import_id, path, module, target_hint
            FROM code_repository_imports
            WHERE source_scope = ?1
            ORDER BY path ASC, import_id ASC
            ",
        )?;
        let rows = statement.query_map(params![member.member.source_scope], |row| {
            Ok(ImportRecord {
                repository_id: row.get(0)?,
                source_scope: row.get(1)?,
                import_id: row.get(2)?,
                path: row.get(3)?,
                module: row.get(4)?,
                target_hint: row.get(5)?,
            })
        })?;
        imports.extend(rows.collect::<Result<Vec<_>, _>>()?);
    }

    Ok(imports)
}

fn exports_for_members(
    connection: &mut Connection,
    members: &[CodeRepositorySetMemberStatus],
) -> Result<Vec<ExportTarget>, StorageError> {
    let mut exports = Vec::new();
    for member in members {
        let mut file_statement = connection.prepare(
            "
            SELECT repository_id, source_scope, file_id, path
            FROM code_repository_files
            WHERE source_scope = ?1
            ",
        )?;
        let file_rows = file_statement.query_map(params![member.member.source_scope], |row| {
            let path = row.get::<_, String>(3)?;
            Ok(ExportTarget {
                repository_id: row.get(0)?,
                source_scope: row.get(1)?,
                record_kind: "code_file".to_owned(),
                record_id: row.get(2)?,
                keys: module_keys_for_path(&path),
            })
        })?;
        exports.extend(file_rows.collect::<Result<Vec<_>, _>>()?);

        let mut symbol_statement = connection.prepare(
            "
            SELECT repository_id, source_scope, symbol_snapshot_id, name, qualified_name, path
            FROM code_repository_symbols
            WHERE source_scope = ?1
            ",
        )?;
        let symbol_rows =
            symbol_statement.query_map(params![member.member.source_scope], |row| {
                let name = row.get::<_, String>(3)?;
                let qualified_name = row.get::<_, String>(4)?;
                let path = row.get::<_, String>(5)?;
                let mut keys = module_keys_for_path(&path);
                keys.insert(normalize_module_key(&name));
                keys.insert(normalize_module_key(&qualified_name));
                Ok(ExportTarget {
                    repository_id: row.get(0)?,
                    source_scope: row.get(1)?,
                    record_kind: "code_symbol_snapshot".to_owned(),
                    record_id: row.get(2)?,
                    keys,
                })
            })?;
        exports.extend(symbol_rows.collect::<Result<Vec<_>, _>>()?);
    }

    Ok(exports)
}

fn matching_exports(import: &ImportRecord, exports: &[ExportTarget]) -> Vec<ExportTarget> {
    let module = normalize_module_key(&import.module);
    let hint = import
        .target_hint
        .as_deref()
        .map(normalize_module_key)
        .unwrap_or_default();
    let last = module
        .rsplit('.')
        .next()
        .unwrap_or(module.as_str())
        .to_owned();
    let mut scored_candidates = exports
        .iter()
        .filter(|target| target.source_scope != import.source_scope)
        .filter_map(|target| {
            target_match_score(target, &module, &hint, &last).map(|score| (score, target.clone()))
        })
        .collect::<Vec<_>>();
    let Some(best_score) = scored_candidates.iter().map(|(score, _)| *score).max() else {
        return Vec::new();
    };
    let mut candidates = scored_candidates
        .drain(..)
        .filter(|(score, _)| *score == best_score)
        .map(|(_, target)| target)
        .collect::<Vec<_>>();
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

    candidates
}

fn target_match_score(target: &ExportTarget, module: &str, hint: &str, last: &str) -> Option<u8> {
    target
        .keys
        .iter()
        .filter_map(|key| {
            if !hint.is_empty() && key == hint {
                Some(5)
            } else if key == module {
                Some(4)
            } else if !last.is_empty() && key.rsplit('.').next() == Some(last) {
                Some(1)
            } else {
                None
            }
        })
        .max()
}

fn edge_for_import(
    set_id: &str,
    import: &ImportRecord,
    candidates: &[ExportTarget],
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

fn module_keys_for_path(path: &str) -> BTreeSet<String> {
    let without_extension = path
        .rsplit_once('.')
        .map(|(left, _)| left)
        .unwrap_or(path)
        .trim_start_matches("./");
    let normalized = normalize_module_key(without_extension);
    let mut keys = BTreeSet::new();
    keys.insert(normalized.clone());
    if let Some(last) = normalized.rsplit('.').next() {
        keys.insert(last.to_owned());
    }
    keys
}

fn normalize_module_key(value: &str) -> String {
    let mut value = value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(';')
        .trim();
    if let Some(stripped) = value.strip_prefix("use ") {
        value = stripped.trim();
    } else if let Some(stripped) = value.strip_prefix("import ") {
        value = stripped.trim();
    }
    value
        .replace("::", ".")
        .replace(['/', '\\', '-'], ".")
        .replace(['{', '}', ','], ".")
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches('.')
        .to_lowercase()
}

fn set_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositorySet> {
    Ok(CodeRepositorySet {
        set_id: row.get(0)?,
        alias: row.get(1)?,
        description: row.get(2)?,
        default_ref_policy_json: row.get(3)?,
        created_at_ms: row.get(4)?,
        updated_at_ms: row.get(5)?,
    })
}

fn member_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositorySetMember> {
    Ok(CodeRepositorySetMember {
        set_id: row.get(0)?,
        repository_id: row.get(1)?,
        repository_alias: row.get(2)?,
        ref_selector: row.get(3)?,
        resolved_commit_sha: row.get(4)?,
        source_scope: row.get(5)?,
        path_filters: parse_json_list(row.get::<_, String>(6)?)?,
        language_filters: parse_json_list(row.get::<_, String>(7)?)?,
        priority: row.get(8)?,
    })
}

fn member_status_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositorySetMemberStatus> {
    let stale = row.get::<_, i64>(10)? != 0;
    Ok(CodeRepositorySetMemberStatus {
        member: CodeRepositorySetMember {
            set_id: row.get(0)?,
            repository_id: row.get(1)?,
            repository_alias: row.get(2)?,
            ref_selector: row.get(3)?,
            resolved_commit_sha: row.get(4)?,
            source_scope: row.get(5)?,
            path_filters: parse_json_list(row.get::<_, String>(6)?)?,
            language_filters: parse_json_list(row.get::<_, String>(7)?)?,
            priority: row.get(8)?,
        },
        tree_hash: row.get(9)?,
        freshness_state: if stale {
            "stale".to_owned()
        } else {
            "fresh".to_owned()
        },
        stale,
        indexed_file_count: row.get(11)?,
        symbol_count: row.get(12)?,
        reference_count: row.get(13)?,
        chunk_count: row.get(14)?,
        degraded_reason: row.get(15)?,
    })
}

fn edge_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositoryCrossEdge> {
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

fn json_list(values: &[String]) -> Result<String, StorageError> {
    serde_json::to_string(values).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

#[derive(Debug, Clone)]
struct ImportRecord {
    repository_id: String,
    source_scope: String,
    import_id: String,
    path: String,
    module: String,
    target_hint: Option<String>,
}

#[derive(Debug, Clone)]
struct ExportTarget {
    repository_id: String,
    source_scope: String,
    record_kind: String,
    record_id: String,
    keys: BTreeSet<String>,
}
