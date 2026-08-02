//! Repository-set overlay refresh, edge resolution, and status projection.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::json;

use crate::{
    domain::{
        CodeRepositoryCrossEdge, CodeRepositorySetMemberStatus, CodeRepositorySetOverlayStatus,
        CodeRepositorySetRefreshSummary, CodeRepositorySetStatus,
    },
    storage::StorageError,
};

use super::super::super::evidence_identity::stable_id;
use super::{
    manifest::{
        manifest_module_prefixes_for_members, module_keys_for_path_with_prefixes,
        module_keys_for_symbol_path_with_prefixes, normalize_module_key,
    },
    membership::{member_statuses, set_by_alias},
};

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

pub(in super::super) fn refresh_overlay(
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
    let exports = ExportIndex::new(exports_for_members(connection, &status.members)?);
    let mut edges = Vec::new();
    for import in imports {
        if let Some(candidates) = matching_exports(&import, &exports) {
            edges.push(edge_for_import(
                &status.repository_set.set_id,
                &import,
                &candidates,
                now_ms,
            ));
        }
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
        ORDER BY from_source_scope ASC, from_record_id ASC, edge_id ASC
        ",
    )?;
    let rows = statement.query_map(params![set_id], edge_from_row)?;

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
            SELECT repository_id, source_scope, import_id, path, module, target_hint,
                   resolution_state, line_start, line_end
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
                resolution_state: row.get(6)?,
                line_start: row.get(7)?,
                line_end: row.get(8)?,
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
    let module_prefixes = manifest_module_prefixes_for_members(connection, members)?;
    let mut exports = Vec::new();
    for member in members {
        let prefixes = module_prefixes
            .get(&member.member.source_scope)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut file_statement = connection.prepare(
            "
            SELECT repository_id, source_scope, file_id, path
            FROM code_repository_files
            WHERE source_scope = ?1
            ",
        )?;
        let file_rows = file_statement.query_map(params![member.member.source_scope], |row| {
            let path = row.get::<_, String>(3)?;
            let keys = module_keys_for_path_with_prefixes(&path, prefixes);
            Ok(ExportTarget {
                repository_id: row.get(0)?,
                source_scope: row.get(1)?,
                record_kind: "code_file".to_owned(),
                record_id: row.get(2)?,
                keys,
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
                let mut keys = module_keys_for_symbol_path_with_prefixes(&path, prefixes);
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

fn matching_exports(import: &ImportRecord, exports: &ExportIndex) -> Option<Vec<ExportTarget>> {
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

#[derive(Debug, Clone)]
struct ExportTarget {
    repository_id: String,
    source_scope: String,
    record_kind: String,
    record_id: String,
    keys: BTreeSet<String>,
}

struct ExportIndex {
    targets: Vec<ExportTarget>,
    by_key: BTreeMap<String, Vec<usize>>,
}

impl ExportIndex {
    fn new(targets: Vec<ExportTarget>) -> Self {
        let mut by_key = BTreeMap::<String, Vec<usize>>::new();
        for (position, target) in targets.iter().enumerate() {
            for key in &target.keys {
                by_key.entry(key.clone()).or_default().push(position);
            }
        }

        Self { targets, by_key }
    }

    fn matching_targets(&self, import_scope: &str, module: &str) -> Vec<ExportTarget> {
        let exact = self.targets_for_key(module, import_scope);
        if !exact.is_empty() {
            return exact;
        }

        let Some((parent, imported_name)) = module.rsplit_once('.') else {
            return Vec::new();
        };
        if parent.is_empty() || imported_name.is_empty() {
            return Vec::new();
        }

        self.targets_for_key_intersection(parent, imported_name, import_scope)
    }

    fn targets_for_key(&self, key: &str, import_scope: &str) -> Vec<ExportTarget> {
        self.by_key
            .get(key)
            .into_iter()
            .flatten()
            .filter_map(|position| self.target_for_import(*position, import_scope))
            .cloned()
            .collect()
    }

    fn targets_for_key_intersection(
        &self,
        left_key: &str,
        right_key: &str,
        import_scope: &str,
    ) -> Vec<ExportTarget> {
        let Some(left_positions) = self.by_key.get(left_key) else {
            return Vec::new();
        };
        let Some(right_positions) = self.by_key.get(right_key) else {
            return Vec::new();
        };

        let (probe, lookup) = if left_positions.len() <= right_positions.len() {
            (left_positions, right_positions)
        } else {
            (right_positions, left_positions)
        };
        let lookup = lookup.iter().copied().collect::<BTreeSet<_>>();
        probe
            .iter()
            .copied()
            .filter(|position| lookup.contains(position))
            .filter_map(|position| self.target_for_import(position, import_scope))
            .cloned()
            .collect()
    }

    fn target_for_import(&self, position: usize, import_scope: &str) -> Option<&ExportTarget> {
        self.targets
            .get(position)
            .filter(|target| target.source_scope != import_scope)
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
