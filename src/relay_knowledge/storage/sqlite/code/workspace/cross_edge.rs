use rusqlite::{Transaction, params};
use serde_json::json;

use crate::{
    domain::{CodeMonorepoWorkspace, CodeRepositorySet},
    storage::StorageError,
};

use super::super::super::evidence_identity::stable_id;
use super::{
    ecosystem::{ecosystem_for_language, is_local_or_relative_module, workspace_lookup_module},
    mapping::{find_workspace_mapping_target, matches_workspace_package},
    member_target::{
        WorkspaceMemberPathMap, workspace_import_is_from_target_member,
        workspace_member_path_for_target, workspace_member_paths, workspace_target_file_id,
    },
};

type SqlParamBatch = Vec<(String, Vec<Box<dyn rusqlite::types::ToSql>>)>;

struct UnresolvedImport {
    import_id: String,
    module: String,
    path: String,
    language_id: String,
    line_start: u32,
    line_end: u32,
}

/// Replaces cross-workspace edges for one repository scope.
pub(super) fn replace_workspace_cross_edges(
    transaction: &Transaction<'_>,
    workspaces: &[CodeMonorepoWorkspace],
    set: &CodeRepositorySet,
    repository_id: &str,
    source_scope: &str,
    now: u64,
) -> Result<(), StorageError> {
    transaction.execute(
        "DELETE FROM code_repository_cross_edges
         WHERE set_id = ?1 AND from_repository_id = ?2 AND from_source_scope = ?3",
        params![set.set_id, repository_id, source_scope],
    )?;

    let member_paths = workspace_member_paths(workspaces);
    let edges = collect_workspace_cross_edges(
        transaction,
        set,
        source_scope,
        repository_id,
        &member_paths,
        now,
    )?;
    if edges.is_empty() {
        return Ok(());
    }

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
            params.iter().map(|param| param.as_ref()).collect();
        insert_edge.execute(param_refs.as_slice())?;
    }
    Ok(())
}

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
        let matches_package = mapping.is_none()
            && matches_workspace_package(
                transaction,
                &set.set_id,
                lookup_module,
                import_ecosystem,
            )?
            .is_some();

        let (to_scope, to_repository, to_kind, to_id, state, confidence, tier, target_hint) =
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
                None if matches_package => (
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
            Box::new(to_repository),
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

#[cfg(test)]
#[path = "cross_edge_tests.rs"]
mod cross_edge_tests;
