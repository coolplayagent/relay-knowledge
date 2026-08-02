//! Repository-set manifest module-prefix discovery.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{domain::CodeRepositorySetMemberStatus, storage::StorageError};

pub(super) use module_key::{
    ModulePrefix, module_keys_for_path_with_prefixes, module_keys_for_symbol_path_with_prefixes,
    normalize_module_key,
};

mod go;
mod module_key;
mod package;
mod path;

#[derive(Debug, Clone)]
pub(super) struct ManifestChunk {
    pub(super) path: String,
    pub(super) content: String,
}

pub(super) fn manifest_module_prefixes_for_members(
    connection: &mut Connection,
    members: &[CodeRepositorySetMemberStatus],
) -> Result<BTreeMap<String, Vec<ModulePrefix>>, StorageError> {
    let mut prefixes_by_scope = BTreeMap::new();
    for member in members {
        let chunks = manifest_chunks(connection, &member.member.source_scope)?;
        let go_workspaces = go::workspaces(&chunks);
        let pnpm_workspaces = package::workspaces(&chunks);
        let mut prefixes = Vec::new();
        for chunk in &chunks {
            if path::is_go_mod(&chunk.path) && go::module_allowed(&chunk.path, &go_workspaces) {
                go::collect_module_prefixes(&chunk.path, &chunk.content, &mut prefixes);
            } else if path::is_package_json(&chunk.path) {
                package::collect_prefixes(
                    &chunk.path,
                    &chunk.content,
                    &pnpm_workspaces,
                    &mut prefixes,
                );
            }
        }
        if !prefixes.is_empty() {
            prefixes_by_scope.insert(member.member.source_scope.clone(), prefixes);
        }
    }

    Ok(prefixes_by_scope)
}

fn manifest_chunks(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<ManifestChunk>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT path, content
        FROM code_repository_chunks
        WHERE source_scope = ?1
          AND (
              path = 'go.mod' OR path LIKE '%/go.mod'
              OR path = 'go.work' OR path LIKE '%/go.work'
              OR path = 'pnpm-workspace.yaml' OR path LIKE '%/pnpm-workspace.yaml'
              OR path = 'pnpm-workspace.yml' OR path LIKE '%/pnpm-workspace.yml'
              OR path = 'package.json' OR path LIKE '%/package.json'
          )
        ORDER BY path ASC, chunk_id ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(ManifestChunk {
            path: row.get(0)?,
            content: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
