use rusqlite::{Transaction, params};

use crate::{
    domain::{CodeMonorepoWorkspace, CodeRepositorySet},
    storage::StorageError,
};

use super::ecosystem::{ecosystem_for_format, workspace_format_key, workspace_package_candidates};

/// A candidate cross-repository target from the workspace package mapping table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorkspaceMappingTarget {
    pub package_name: String,
    pub ecosystem: String,
    pub repository_id: String,
    pub source_scope: String,
}

/// Replaces package mappings so removed workspace members cannot remain resolvable.
pub(super) fn replace_workspace_package_mappings(
    transaction: &Transaction<'_>,
    workspaces: &[CodeMonorepoWorkspace],
    set: &CodeRepositorySet,
    repository_id: &str,
    source_scope: &str,
    now: u64,
) -> Result<(), StorageError> {
    if workspaces.is_empty() {
        return Ok(());
    }

    transaction.execute(
        "DELETE FROM code_workspace_package_mappings WHERE set_id = ?1",
        params![set.set_id],
    )?;

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

/// Finds the longest package prefix in the matching ecosystem.
pub(super) fn find_workspace_mapping_target(
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
    params_builder.extend(
        candidates
            .iter()
            .cloned()
            .map(|candidate| Box::new(candidate) as Box<dyn rusqlite::types::ToSql>),
    );
    params_builder.push(Box::new(ecosystem.to_owned()));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_builder.iter().map(|param| param.as_ref()).collect();

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

/// Reports a known package even when its target scope is not fully indexed.
pub(super) fn matches_workspace_package(
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
    params_builder.extend(
        candidates
            .iter()
            .cloned()
            .map(|candidate| Box::new(candidate) as Box<dyn rusqlite::types::ToSql>),
    );
    params_builder.push(Box::new(ecosystem.to_owned()));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_builder.iter().map(|param| param.as_ref()).collect();

    Ok(statement
        .query_row(param_refs.as_slice(), |row| row.get(0))
        .ok())
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod mapping_tests;
