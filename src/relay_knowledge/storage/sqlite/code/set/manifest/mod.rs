//! Repository-set manifest module-prefix discovery.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{domain::CodeRepositorySetMemberStatus, storage::StorageError};

use super::capacity::{
    MAX_REPOSITORY_SET_MANIFEST_BYTES, MAX_REPOSITORY_SET_MANIFEST_CHUNKS,
    MAX_REPOSITORY_SET_MANIFEST_ITEMS, capacity_error,
};

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
    let mut remaining_chunks = MAX_REPOSITORY_SET_MANIFEST_CHUNKS;
    let mut remaining_bytes = MAX_REPOSITORY_SET_MANIFEST_BYTES;
    let mut remaining_items = MAX_REPOSITORY_SET_MANIFEST_ITEMS;
    for member in members {
        let chunks = manifest_chunks(
            connection,
            &member.member.source_scope,
            remaining_chunks,
            remaining_bytes,
        )?;
        consume_budget(
            &mut remaining_chunks,
            chunks.len(),
            "manifest chunk",
            MAX_REPOSITORY_SET_MANIFEST_CHUNKS,
        )?;
        let chunk_bytes = chunks.iter().try_fold(0usize, |total, chunk| {
            total.checked_add(chunk.path.len().saturating_add(chunk.content.len()))
        });
        consume_budget(
            &mut remaining_bytes,
            chunk_bytes.unwrap_or(usize::MAX),
            "manifest byte",
            MAX_REPOSITORY_SET_MANIFEST_BYTES,
        )?;
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
        let item_count = module_key::module_prefix_item_count(&prefixes);
        consume_budget(
            &mut remaining_items,
            item_count,
            "manifest-derived item",
            MAX_REPOSITORY_SET_MANIFEST_ITEMS,
        )?;
        if !prefixes.is_empty() {
            prefixes_by_scope.insert(member.member.source_scope.clone(), prefixes);
        }
    }

    Ok(prefixes_by_scope)
}

fn consume_budget(
    remaining: &mut usize,
    observed: usize,
    kind: &str,
    capacity: usize,
) -> Result<(), StorageError> {
    let Some(next) = remaining.checked_sub(observed) else {
        return Err(capacity_error(kind, capacity));
    };
    *remaining = next;
    Ok(())
}

fn manifest_chunks(
    connection: &Connection,
    source_scope: &str,
    remaining_chunks: usize,
    remaining_bytes: usize,
) -> Result<Vec<ManifestChunk>, StorageError> {
    if remaining_chunks == 0 {
        let exists = connection.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_chunks
                 WHERE source_scope = ?1 AND (
                     path = 'go.mod' OR path LIKE '%/go.mod'
                     OR path = 'go.work' OR path LIKE '%/go.work'
                     OR path = 'pnpm-workspace.yaml' OR path LIKE '%/pnpm-workspace.yaml'
                     OR path = 'pnpm-workspace.yml' OR path LIKE '%/pnpm-workspace.yml'
                     OR path = 'package.json' OR path LIKE '%/package.json'
                 ) LIMIT 1
             )",
            params![source_scope],
            |row| row.get::<_, bool>(0),
        )?;
        if exists {
            return Err(capacity_error(
                "manifest chunk",
                MAX_REPOSITORY_SET_MANIFEST_CHUNKS,
            ));
        }
        return Ok(Vec::new());
    }
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
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(params![source_scope, remaining_chunks + 1], |row| {
        Ok(ManifestChunk {
            path: row.get(0)?,
            content: row.get(1)?,
        })
    })?;
    let chunks = rows.collect::<Result<Vec<_>, _>>()?;
    if chunks.len() > remaining_chunks {
        return Err(capacity_error(
            "manifest chunk",
            MAX_REPOSITORY_SET_MANIFEST_CHUNKS,
        ));
    }
    let observed_bytes = chunks.iter().try_fold(0usize, |total, chunk| {
        total.checked_add(chunk.path.len().saturating_add(chunk.content.len()))
    });
    if observed_bytes.is_none_or(|bytes| bytes > remaining_bytes) {
        return Err(capacity_error(
            "manifest byte",
            MAX_REPOSITORY_SET_MANIFEST_BYTES,
        ));
    }
    Ok(chunks)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
