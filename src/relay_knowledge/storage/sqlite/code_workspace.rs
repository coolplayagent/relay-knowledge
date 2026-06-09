use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::json;

use crate::{
    domain::{CodeMonorepoWorkspace, CodeMonorepoWorkspaceFormat, CodeRepositorySet},
    storage::StorageError,
};

use super::super::helpers::stable_id;

/// Batch of SQL parameter sets keyed by edge ID for batch insertion.
type SqlParamBatch = Vec<(String, Vec<Box<dyn rusqlite::types::ToSql>>)>;
type WorkspaceMemberPathMap = BTreeMap<(String, String), String>;

// ── workspace set management ──────────────────────────────────────────

pub(super) fn workspace_set_id(repository_id: &str) -> String {
    stable_id("code-auto-workspace-set", repository_id)
}

/// Creates or retrieves a workspace-scoped repository set for the given
/// repository, inserting it into `code_repository_sets` and
/// `code_repository_set_members` if it does not already exist.
fn ensure_workspace_set(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<CodeRepositorySet, StorageError> {
    let set_id = workspace_set_id(repository_id);
    let now = now_millis();
    let scope = workspace_scope_metadata(transaction, source_scope)?;

    let set = CodeRepositorySet {
        set_id: set_id.clone(),
        alias: auto_workspace_set_alias(repository_id),
        description: Some(format!(
            "Auto-detected monorepo workspace set for {repository_id}"
        )),
        default_ref_policy_json: json!({"default_ref": "HEAD"}).to_string(),
        created_at_ms: now,
        updated_at_ms: now,
    };

    transaction.execute(
        "INSERT INTO code_repository_sets
         (set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(set_id) DO UPDATE SET
            alias = excluded.alias,
            description = excluded.description,
            default_ref_policy_json = excluded.default_ref_policy_json,
            updated_at_ms = excluded.updated_at_ms",
        params![
            &set.set_id,
            &set.alias,
            &set.description,
            &set.default_ref_policy_json,
            set.created_at_ms,
            set.updated_at_ms,
        ],
    )?;

    transaction.execute(
        "INSERT OR REPLACE INTO code_repository_set_members
         (set_id, repository_id, repository_alias, ref_selector,
          resolved_commit_sha, source_scope, path_filters_json,
          language_filters_json, priority)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
        params![
            set.set_id,
            repository_id,
            repository_id,
            scope.resolved_commit_sha,
            scope.resolved_commit_sha,
            source_scope,
            scope.path_filters_json,
            scope.language_filters_json
        ],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
        params![set.set_id],
    )?;

    Ok(set)
}

pub(super) fn clear_auto_workspace_state(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    clear_workspace_state(&transaction, repository_id, source_scope)?;
    transaction.commit()?;
    Ok(())
}

fn auto_workspace_set_alias(repository_id: &str) -> String {
    format!("{repository_id}-auto-workspace")
}

struct WorkspaceScopeMetadata {
    resolved_commit_sha: String,
    path_filters_json: String,
    language_filters_json: String,
}

fn workspace_scope_metadata(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<WorkspaceScopeMetadata, StorageError> {
    transaction
        .query_row(
            "
            SELECT resolved_commit_sha, path_filters_json, language_filters_json
            FROM code_repository_scopes
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| {
                Ok(WorkspaceScopeMetadata {
                    resolved_commit_sha: row.get(0)?,
                    path_filters_json: row.get(1)?,
                    language_filters_json: row.get(2)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "workspace source scope '{source_scope}' is not published"
            ))
        })
}

// ── package mapping population ────────────────────────────────────────

/// Populates or refreshes workspace package mappings from detected workspace data.
///
/// For each workspace and each member, an entry is upserted into
/// `code_workspace_package_mappings` so that later cross-repo import
/// resolution can translate package names into indexed scopes.
pub(crate) fn upsert_workspace_package_mappings(
    transaction: &Transaction<'_>,
    workspaces: &[CodeMonorepoWorkspace],
    set: &CodeRepositorySet,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    if workspaces.is_empty() {
        return Ok(());
    }

    transaction.execute(
        "DELETE FROM code_workspace_package_mappings WHERE set_id = ?1",
        params![set.set_id],
    )?;

    let now = now_millis();
    let mut statement = transaction.prepare(
        "
        INSERT INTO code_workspace_package_mappings
            (set_id, package_name, ecosystem, repository_id, source_scope,
             workspace_format, created_at_ms)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(set_id, package_name, ecosystem) DO UPDATE SET
            repository_id = excluded.repository_id,
            source_scope = excluded.source_scope,
            workspace_format = excluded.workspace_format,
            created_at_ms = excluded.created_at_ms
        ",
    )?;

    for workspace in workspaces {
        let ecosystem = ecosystem_for_format(workspace.format);
        let workspace_format = workspace_format_key(workspace.format);
        for member in &workspace.members {
            if member.package_name.is_empty() {
                continue;
            }
            statement.execute(params![
                set.set_id,
                member.package_name,
                ecosystem,
                repository_id,
                source_scope,
                workspace_format,
                now,
            ])?;
        }
    }

    Ok(())
}

// ── mapping lookup ────────────────────────────────────────────────────

/// A candidate cross-repo target resolved from the workspace package mapping
/// table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorkspaceMappingTarget {
    pub package_name: String,
    pub ecosystem: String,
    pub repository_id: String,
    pub source_scope: String,
}

const WORKSPACE_PACKAGE_SEPARATORS: [&str; 3] = ["::", "/", "."];

fn workspace_package_candidates(import_module: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut remaining = import_module.trim();
    if remaining.is_empty() {
        return candidates;
    }

    loop {
        if !candidates.iter().any(|candidate| candidate == remaining) {
            candidates.push(remaining.to_owned());
        }

        match rightmost_package_separator(remaining) {
            Some(separator_index) if separator_index > 0 => {
                remaining = &remaining[..separator_index];
            }
            _ => break,
        }
    }

    candidates
}

fn rightmost_package_separator(value: &str) -> Option<usize> {
    WORKSPACE_PACKAGE_SEPARATORS
        .iter()
        .filter_map(|separator| value.rfind(separator))
        .max()
}

/// Queries the workspace mapping table for a target that matches the given
/// import module via prefix-based matching.
///
/// Prefix matching handles cases where a Go-style import like
/// `example.com/svc/api/client` maps to a workspace package
/// `example.com/svc/api`.  The longest-matching prefix wins.
fn find_workspace_mapping_target(
    transaction: &Transaction<'_>,
    set_id: &str,
    import_module: &str,
    ecosystem: &str,
) -> Result<Option<WorkspaceMappingTarget>, StorageError> {
    let candidates = workspace_package_candidates(import_module);
    if candidates.is_empty() {
        return Ok(None);
    }

    let placeholders: Vec<String> = (0..candidates.len())
        .map(|index| format!("?{}", index + 2))
        .collect();
    let sql = format!(
        "SELECT package_name, ecosystem, repository_id, source_scope
         FROM code_workspace_package_mappings
         WHERE set_id = ?1 AND package_name IN ({}) AND ecosystem = ?{}
         ORDER BY LENGTH(package_name) DESC
         LIMIT 1",
        placeholders.join(", "),
        candidates.len() + 2
    );

    let mut statement = transaction.prepare(&sql)?;
    let mut params_builder: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params_builder.push(Box::new(set_id.to_owned()));
    for candidate in &candidates {
        params_builder.push(Box::new(candidate.clone()));
    }
    params_builder.push(Box::new(ecosystem.to_owned()));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_builder.iter().map(|p| p.as_ref()).collect();

    Ok(statement
        .query_row(param_refs.as_slice(), |row| {
            Ok(WorkspaceMappingTarget {
                package_name: row.get(0)?,
                ecosystem: row.get(1)?,
                repository_id: row.get(2)?,
                source_scope: row.get(3)?,
            })
        })
        .ok())
}

/// Checks whether the import module matches any known workspace package name
/// in the mapping table, without requiring the target to be fully indexed.
fn matches_workspace_package(
    transaction: &Transaction<'_>,
    set_id: &str,
    import_module: &str,
    ecosystem: &str,
) -> Result<Option<String>, StorageError> {
    let candidates = workspace_package_candidates(import_module);
    if candidates.is_empty() {
        return Ok(None);
    }

    let placeholders: Vec<String> = (0..candidates.len())
        .map(|index| format!("?{}", index + 2))
        .collect();
    let sql = format!(
        "SELECT package_name
         FROM code_workspace_package_mappings
         WHERE set_id = ?1 AND package_name IN ({}) AND ecosystem = ?{}
         ORDER BY LENGTH(package_name) DESC
         LIMIT 1",
        placeholders.join(", "),
        candidates.len() + 2
    );

    let mut statement = transaction.prepare(&sql)?;
    let mut params_builder: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params_builder.push(Box::new(set_id.to_owned()));
    for candidate in &candidates {
        params_builder.push(Box::new(candidate.clone()));
    }
    params_builder.push(Box::new(ecosystem.to_owned()));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_builder.iter().map(|p| p.as_ref()).collect();

    Ok(statement
        .query_row(param_refs.as_slice(), |row| row.get(0))
        .ok())
}

// ── cross-repo import resolution ──────────────────────────────────────

/// A lightweight import loaded for workspace cross-repo resolution during indexing.
struct UnresolvedImport {
    import_id: String,
    module: String,
    path: String,
    language_id: String,
    line_start: u32,
    line_end: u32,
}

/// Collects cross-repo edges for unresolved imports by matching them
/// against the workspace package mapping table.
fn collect_workspace_cross_edges(
    transaction: &Transaction<'_>,
    set: &CodeRepositorySet,
    source_scope: &str,
    repository_id: &str,
    member_paths: &WorkspaceMemberPathMap,
    now: u64,
) -> Result<SqlParamBatch, StorageError> {
    let imports = load_workspace_resolvable_imports(transaction, source_scope)?;
    if imports.is_empty() {
        return Ok(Vec::new());
    }

    let mut edges = Vec::new();

    for import in &imports {
        let Some(import_ecosystem) = ecosystem_for_language(&import.language_id) else {
            continue;
        };
        let lookup_module = workspace_lookup_module(&import.module, import_ecosystem);
        if is_local_or_relative_module(lookup_module) {
            continue;
        }
        let mapping = find_workspace_mapping_target(
            transaction,
            &set.set_id,
            lookup_module,
            import_ecosystem,
        )?;
        let matches_pkg = mapping.is_none()
            && matches_workspace_package(
                transaction,
                &set.set_id,
                lookup_module,
                import_ecosystem,
            )?
            .is_some();

        let (to_scope, to_repo, to_kind, to_id, state, confidence, tier, target_hint) =
            match mapping {
                Some(target) => {
                    let member_path = workspace_member_path_for_target(member_paths, &target);
                    if workspace_import_is_from_target_member(member_paths, &target, &import.path) {
                        continue;
                    }
                    let target_id = workspace_target_file_id(transaction, &target, member_path)?;
                    (
                        Some(target.source_scope),
                        Some(target.repository_id),
                        "code_file".to_owned(),
                        target_id,
                        "resolved",
                        10_000u16,
                        "explicit",
                        format!("{} ({})", target.package_name, target.ecosystem),
                    )
                }
                None if matches_pkg => (
                    None,
                    None,
                    "unresolved_target".to_owned(),
                    None,
                    "unresolved",
                    0u16,
                    "unresolved",
                    lookup_module.to_owned(),
                ),
                None => continue,
            };

        let edge_id = stable_id(
            "code-repository-cross-edge",
            &format!(
                "{}:{}:{}:{}:{}",
                set.set_id, source_scope, import.import_id, lookup_module, state
            ),
        );
        let evidence_json = json!({
            "module": import.module,
            "target_hint": target_hint,
            "from_path": import.path,
            "from_line_start": import.line_start,
            "from_line_end": import.line_end,
            "candidate_count": 1u32,
        })
        .to_string();

        let params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(edge_id.clone()),
            Box::new(set.set_id.clone()),
            Box::new(source_scope.to_owned()),
            Box::new(repository_id.to_owned()),
            Box::new("module_reference".to_owned()),
            Box::new(import.import_id.clone()),
            Box::new(to_scope),
            Box::new(to_repo),
            Box::new(to_kind),
            Box::new(to_id),
            Box::new("cross_repo_import".to_owned()),
            Box::new(state.to_owned()),
            Box::new(confidence),
            Box::new(tier.to_owned()),
            Box::new(evidence_json),
            Box::new(now),
        ];
        edges.push((edge_id, params));
    }

    Ok(edges)
}

/// Resolves unresolved imports against workspace package mappings and
/// creates cross-repository edges in `code_repository_cross_edges`.
///
/// Empty `workspaces` clears any previous auto-detected workspace state so
/// a later index cannot keep stale package mappings or generated edges.
pub(crate) fn resolve_workspace_imports(
    transaction: &Transaction<'_>,
    workspaces: &[CodeMonorepoWorkspace],
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    if workspaces.is_empty() {
        clear_workspace_state(transaction, repository_id, source_scope)?;
        return Ok(());
    }

    let set = ensure_workspace_set(transaction, repository_id, source_scope)?;
    upsert_workspace_package_mappings(transaction, workspaces, &set, repository_id, source_scope)?;
    transaction.execute(
        "DELETE FROM code_repository_cross_edges
         WHERE set_id = ?1 AND from_repository_id = ?2 AND from_source_scope = ?3",
        params![set.set_id, repository_id, source_scope],
    )?;

    let now = now_millis();
    let member_paths = workspace_member_paths(workspaces);
    let edges = collect_workspace_cross_edges(
        transaction,
        &set,
        source_scope,
        repository_id,
        &member_paths,
        now,
    )?;
    if !edges.is_empty() {
        let mut insert_edge = transaction.prepare(
            "
            INSERT INTO code_repository_cross_edges (
                edge_id, set_id, from_source_scope, from_repository_id, from_record_kind,
                from_record_id, to_source_scope, to_repository_id, to_record_kind, to_record_id,
                edge_kind, resolution_state, confidence_basis_points, confidence_tier,
                evidence_json, created_at_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ",
        )?;

        for (_edge_id, params) in &edges {
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            insert_edge.execute(param_refs.as_slice())?;
        }
    }

    refresh_workspace_overlay_status(transaction, &set.set_id, now)?;

    Ok(())
}

pub(super) fn clear_workspace_state(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let set_id = workspace_set_id(repository_id);
    transaction.execute(
        "DELETE FROM code_repository_cross_edges
         WHERE set_id = ?1 AND from_repository_id = ?2 AND from_source_scope = ?3",
        params![&set_id, repository_id, source_scope],
    )?;
    transaction.execute(
        "DELETE FROM code_workspace_package_mappings
         WHERE set_id = ?1 AND source_scope = ?2",
        params![&set_id, source_scope],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_set_members
         WHERE set_id = ?1 AND repository_id = ?2 AND source_scope = ?3",
        params![&set_id, repository_id, source_scope],
    )?;

    let remaining_members: usize = transaction.query_row(
        "SELECT COUNT(*) FROM code_repository_set_members WHERE set_id = ?1",
        params![&set_id],
        |row| row.get(0),
    )?;
    if remaining_members == 0 {
        transaction.execute(
            "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
            params![&set_id],
        )?;
        transaction.execute(
            "DELETE FROM code_repository_sets WHERE set_id = ?1",
            params![&set_id],
        )?;
    } else {
        refresh_workspace_overlay_status(transaction, &set_id, now_millis())?;
    }

    Ok(())
}

fn refresh_workspace_overlay_status(
    transaction: &Transaction<'_>,
    set_id: &str,
    now: u64,
) -> Result<(), StorageError> {
    let edge_count = workspace_cross_edge_count(transaction, set_id)?;
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
            set_id,
            now,
            edge_count,
            workspace_member_versions_json(transaction, set_id)?,
        ],
    )?;
    Ok(())
}

fn workspace_cross_edge_count(
    transaction: &Transaction<'_>,
    set_id: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_cross_edges edge
            WHERE edge.set_id = ?1
              AND EXISTS (
                  SELECT 1
                  FROM code_repository_set_members member
                  WHERE member.set_id = edge.set_id
                    AND member.source_scope = edge.from_source_scope
              )
            ",
            params![set_id],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

fn workspace_member_versions_json(
    transaction: &Transaction<'_>,
    set_id: &str,
) -> Result<String, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT member.repository_id, member.source_scope, member.resolved_commit_sha,
               scope.tree_hash
        FROM code_repository_set_members member
        JOIN code_repository_scopes scope ON scope.source_scope = member.source_scope
        WHERE member.set_id = ?1
        ORDER BY member.repository_alias ASC, member.source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![set_id], |row| {
        Ok(json!({
            "repository_id": row.get::<_, String>(0)?,
            "source_scope": row.get::<_, String>(1)?,
            "resolved_commit_sha": row.get::<_, String>(2)?,
            "tree_hash": row.get::<_, String>(3)?,
            "stale": false,
        }))
    })?;
    let versions = rows.collect::<Result<Vec<_>, _>>()?;
    serde_json::to_string(&versions).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn load_workspace_resolvable_imports(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Vec<UnresolvedImport>, StorageError> {
    let mut statement = transaction.prepare(
        "SELECT imports.import_id, imports.module, imports.path, files.language_id,
                imports.line_start, imports.line_end
         FROM code_repository_imports imports
         INNER JOIN code_repository_files files
            ON files.source_scope = imports.source_scope
           AND files.file_id = imports.file_id
         WHERE imports.source_scope = ?1
           AND imports.resolution_state IN ('unresolved', 'ambiguous')",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(UnresolvedImport {
            import_id: row.get(0)?,
            module: row.get(1)?,
            path: row.get(2)?,
            language_id: row.get(3)?,
            line_start: row.get(4)?,
            line_end: row.get(5)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn workspace_member_paths(workspaces: &[CodeMonorepoWorkspace]) -> WorkspaceMemberPathMap {
    let mut paths = BTreeMap::new();
    for workspace in workspaces {
        let ecosystem = ecosystem_for_format(workspace.format).to_owned();
        for member in &workspace.members {
            if member.package_name.is_empty() {
                continue;
            }
            if let Some(path) = normalized_workspace_member_path(&member.relative_path) {
                paths.insert((member.package_name.clone(), ecosystem.clone()), path);
            }
        }
    }
    paths
}

fn workspace_member_path_for_target<'a>(
    member_paths: &'a WorkspaceMemberPathMap,
    target: &WorkspaceMappingTarget,
) -> Option<&'a str> {
    member_paths
        .get(&(target.package_name.clone(), target.ecosystem.clone()))
        .map(String::as_str)
}

fn workspace_import_is_from_target_member(
    member_paths: &WorkspaceMemberPathMap,
    target: &WorkspaceMappingTarget,
    import_path: &str,
) -> bool {
    let Some(target_path) = workspace_member_path_for_target(member_paths, target) else {
        return false;
    };
    let Some(import_path) = normalized_workspace_member_path(import_path) else {
        return false;
    };

    if !target_path.is_empty() {
        return workspace_path_contains_file(target_path, &import_path);
    }

    !member_paths
        .iter()
        .any(|((_package_name, ecosystem), member_path)| {
            ecosystem == &target.ecosystem
                && !member_path.is_empty()
                && workspace_path_contains_file(member_path, &import_path)
        })
}

fn workspace_path_contains_file(member_path: &str, file_path: &str) -> bool {
    file_path == member_path
        || file_path
            .strip_prefix(member_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn workspace_target_file_id(
    transaction: &Transaction<'_>,
    target: &WorkspaceMappingTarget,
    member_path: Option<&str>,
) -> Result<Option<String>, StorageError> {
    if let Some(member_path) = member_path.and_then(normalized_workspace_member_path) {
        if member_path.is_empty() {
            return workspace_root_target_file_id(transaction, target);
        }
        return workspace_member_target_file_id(transaction, target, &member_path);
    }

    transaction
        .query_row(
            "SELECT file_id
             FROM code_repository_files
             WHERE source_scope = ?1
             ORDER BY
                CASE
                    WHEN path IN ('package.json', 'Cargo.toml', 'go.mod') THEN 0
                    WHEN path LIKE '%/package.json'
                      OR path LIKE '%/Cargo.toml'
                      OR path LIKE '%/go.mod' THEN 1
                    ELSE 2
                END,
                path
             LIMIT 1",
            params![target.source_scope],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn workspace_member_target_file_id(
    transaction: &Transaction<'_>,
    target: &WorkspaceMappingTarget,
    member_path: &str,
) -> Result<Option<String>, StorageError> {
    let child_pattern = format!("{}%", escape_sql_like(&format!("{member_path}/")));
    let package_json = format!("{member_path}/package.json");
    let cargo_toml = format!("{member_path}/Cargo.toml");
    let go_mod = format!("{member_path}/go.mod");
    let preferred_manifest = workspace_manifest_file_name(&target.ecosystem)
        .map(|file_name| format!("{member_path}/{file_name}"));

    transaction
        .query_row(
            "SELECT file_id
             FROM code_repository_files
             WHERE source_scope = ?1
               AND (path = ?2 OR path LIKE ?3 ESCAPE '\\')
             ORDER BY
                CASE
                    WHEN path = ?4 THEN 0
                    WHEN path = ?5 OR path = ?6 OR path = ?7 THEN 1
                    ELSE 2
                END,
                path
             LIMIT 1",
            params![
                target.source_scope,
                member_path,
                child_pattern,
                preferred_manifest,
                package_json,
                cargo_toml,
                go_mod
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn workspace_root_target_file_id(
    transaction: &Transaction<'_>,
    target: &WorkspaceMappingTarget,
) -> Result<Option<String>, StorageError> {
    let Some(manifest_file_name) = workspace_manifest_file_name(&target.ecosystem) else {
        return Ok(None);
    };
    transaction
        .query_row(
            "SELECT file_id
             FROM code_repository_files
             WHERE source_scope = ?1 AND path = ?2
             LIMIT 1",
            params![target.source_scope, manifest_file_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn workspace_manifest_file_name(ecosystem: &str) -> Option<&'static str> {
    match ecosystem {
        "npm" => Some("package.json"),
        "go" => Some("go.mod"),
        "rust" => Some("Cargo.toml"),
        _ => None,
    }
}

fn normalized_workspace_member_path(path: &str) -> Option<String> {
    let replaced = path.trim().replace('\\', "/");
    let mut segments = Vec::new();
    for segment in replaced.split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return None;
        }
        segments.push(segment);
    }
    Some(segments.join("/"))
}

fn escape_sql_like(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn is_local_or_relative_module(module: &str) -> bool {
    let trimmed = module.trim();
    trimmed.is_empty()
        || matches!(trimmed, "crate" | "self" | "super")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with("crate::")
        || trimmed.starts_with("self::")
        || trimmed.starts_with("super::")
}

pub(super) fn workspace_lookup_module<'a>(module: &'a str, ecosystem: &str) -> &'a str {
    let trimmed = module.trim().trim_end_matches(';').trim();
    match ecosystem {
        "go" => go_workspace_lookup_module(trimmed),
        "npm" => npm_workspace_lookup_module(trimmed),
        "rust" => rust_workspace_lookup_module(trimmed),
        _ => trimmed,
    }
}

fn go_workspace_lookup_module(module: &str) -> &str {
    module
        .split_whitespace()
        .last()
        .unwrap_or(module)
        .trim_end_matches(';')
        .trim_matches(|ch| matches!(ch, '"' | '`' | '\''))
        .trim()
}

fn npm_workspace_lookup_module(module: &str) -> &str {
    quoted_workspace_specifier(module).unwrap_or(module).trim()
}

fn rust_workspace_lookup_module(module: &str) -> &str {
    let mut value = module;
    value = value.strip_prefix("pub use ").unwrap_or(value);
    value = value.strip_prefix("use ").unwrap_or(value);
    value = value.strip_prefix("extern crate ").unwrap_or(value);
    let end = value.find([' ', ';', '{']).unwrap_or(value.len());
    value[..end].trim().trim_end_matches("::").trim()
}

fn quoted_workspace_specifier(statement: &str) -> Option<&str> {
    let start = statement.find(['"', '\'', '`'])?;
    let quote = statement.as_bytes()[start] as char;
    let rest = &statement[start + 1..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

// ── helpers ───────────────────────────────────────────────────────────

fn ecosystem_for_format(format: CodeMonorepoWorkspaceFormat) -> &'static str {
    match format {
        CodeMonorepoWorkspaceFormat::Pnpm => "npm",
        CodeMonorepoWorkspaceFormat::GoModules => "go",
        CodeMonorepoWorkspaceFormat::CargoWorkspace => "rust",
    }
}

fn ecosystem_for_language(language_id: &str) -> Option<&'static str> {
    match language_id {
        "javascript" | "jsx" | "typescript" | "tsx" => Some("npm"),
        "go" => Some("go"),
        "rust" => Some("rust"),
        _ => None,
    }
}

fn workspace_format_key(format: CodeMonorepoWorkspaceFormat) -> &'static str {
    match format {
        CodeMonorepoWorkspaceFormat::Pnpm => "pnpm",
        CodeMonorepoWorkspaceFormat::GoModules => "go_modules",
        CodeMonorepoWorkspaceFormat::CargoWorkspace => "cargo_workspace",
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
#[path = "code_workspace_tests.rs"]
mod tests;
