use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{domain::CodeMonorepoWorkspace, storage::StorageError};

use super::{
    ecosystem::{ecosystem_for_format, workspace_manifest_file_name},
    mapping::WorkspaceMappingTarget,
};

pub(super) type WorkspaceMemberPathMap = BTreeMap<(String, String), String>;

pub(super) fn workspace_member_paths(
    workspaces: &[CodeMonorepoWorkspace],
) -> WorkspaceMemberPathMap {
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

pub(super) fn workspace_member_path_for_target<'a>(
    member_paths: &'a WorkspaceMemberPathMap,
    target: &WorkspaceMappingTarget,
) -> Option<&'a str> {
    member_paths
        .get(&(target.package_name.clone(), target.ecosystem.clone()))
        .map(String::as_str)
}

pub(super) fn workspace_import_is_from_target_member(
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

pub(super) fn workspace_target_file_id(
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

pub(super) fn normalized_workspace_member_path(path: &str) -> Option<String> {
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

fn workspace_path_contains_file(member_path: &str, file_path: &str) -> bool {
    file_path == member_path
        || file_path
            .strip_prefix(member_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn escape_sql_like(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
#[path = "member_target_tests.rs"]
mod member_target_tests;
