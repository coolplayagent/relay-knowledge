use rusqlite::{OptionalExtension, params, params_from_iter, types::Value};

use crate::storage::StorageError;

use super::super::search::{delete_search_documents_for_paths, delete_search_documents_for_scope};

const MAX_PATH_DELETE_PATHS_PER_STATEMENT: usize = 500;

pub(in crate::storage::sqlite::code) fn delete_scope_index(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    for table in [
        "code_repository_path_tombstones",
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_routes",
        "code_repository_feature_flags",
        "code_repository_framework_edges",
        "code_repository_framework_nodes",
        "code_repository_dependencies",
        "code_repository_imports",
        "code_repository_reference_search_groups",
        "code_repository_reference_search_manifests",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
        "software_components",
        "software_dependency_usages",
        "software_sdk_usages",
        "software_files",
        "software_topics",
        "software_relationships",
        "software_global_status",
        "software_build_targets",
        "software_iac_resources",
        "software_design_elements",
        "business_mappings",
        "business_term_aliases",
        "business_terms",
        "business_domains",
        "business_knowledge_status",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
        )?;
    }
    for table in [
        "software_entities",
        "software_statements",
        "software_ontology_diagnostics",
    ] {
        delete_optional_projection_rows(transaction, table, source_scope)?;
    }
    delete_search_documents_for_scope(transaction, source_scope)?;

    Ok(())
}

/// Deletes a parallel-release projection when an older database has not created it yet.
fn delete_optional_projection_rows(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    source_scope: &str,
) -> Result<(), StorageError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1
         )",
        params![table],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
        )?;
    }
    Ok(())
}

pub(in crate::storage::sqlite::code) fn path_indexes_exist<'path>(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    paths: impl IntoIterator<Item = &'path str>,
) -> Result<bool, StorageError> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(false);
    }

    for path_chunk in paths.chunks(MAX_PATH_DELETE_PATHS_PER_STATEMENT) {
        let placeholders = std::iter::repeat_n("?", path_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Vec::with_capacity(path_chunk.len() + 1);
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(
            path_chunk
                .iter()
                .map(|path| Value::Text((*path).to_owned())),
        );
        let existing = transaction
            .query_row(
                &format!(
                    "SELECT 1 FROM code_repository_files WHERE source_scope = ? AND path IN ({placeholders}) LIMIT 1"
                ),
                params_from_iter(values),
                |_| Ok(()),
            )
            .optional()?;
        if existing.is_some() {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(in crate::storage::sqlite::code) fn delete_path_indexes<'path>(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    paths: impl IntoIterator<Item = &'path str>,
) -> Result<(), StorageError> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(());
    }

    for path_chunk in paths.chunks(MAX_PATH_DELETE_PATHS_PER_STATEMENT) {
        decrement_reference_search_manifest_for_paths(transaction, source_scope, path_chunk)?;
    }

    for table in [
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_routes",
        "code_repository_feature_flags",
        "code_repository_framework_edges",
        "code_repository_framework_nodes",
        "code_repository_dependencies",
        "code_repository_imports",
        "code_repository_reference_search_groups",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
    ] {
        for path_chunk in paths.chunks(MAX_PATH_DELETE_PATHS_PER_STATEMENT) {
            let placeholders = std::iter::repeat_n("?", path_chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let mut values = Vec::with_capacity(path_chunk.len() + 1);
            values.push(Value::Text(source_scope.to_owned()));
            values.extend(
                path_chunk
                    .iter()
                    .map(|path| Value::Text((*path).to_owned())),
            );
            transaction.execute(
                &format!("DELETE FROM {table} WHERE source_scope = ? AND path IN ({placeholders})"),
                params_from_iter(values),
            )?;
        }
    }
    delete_search_documents_for_paths(transaction, source_scope, paths)?;

    Ok(())
}

fn decrement_reference_search_manifest_for_paths(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    paths: &[&str],
) -> Result<(), StorageError> {
    let placeholders = std::iter::repeat_n("?", paths.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut values = Vec::with_capacity(paths.len() + 1);
    values.push(Value::Text(source_scope.to_owned()));
    values.extend(paths.iter().map(|path| Value::Text((*path).to_owned())));
    let reference_count = transaction.query_row(
        &format!(
            "SELECT COUNT(*) FROM code_repository_references
             WHERE source_scope = ? AND path IN ({placeholders})"
        ),
        params_from_iter(values.clone()),
        |row| row.get::<_, usize>(0),
    )?;
    let group_count = transaction.query_row(
        &format!(
            "SELECT COUNT(*) FROM code_repository_reference_search_groups
             WHERE source_scope = ? AND path IN ({placeholders})"
        ),
        params_from_iter(values),
        |row| row.get::<_, usize>(0),
    )?;
    let manifest = transaction
        .query_row(
            "SELECT reference_count, group_count
             FROM code_repository_reference_search_manifests WHERE source_scope = ?1",
            params![source_scope],
            |row| Ok((row.get::<_, usize>(0)?, row.get::<_, usize>(1)?)),
        )
        .optional()?;
    let Some((manifest_references, manifest_groups)) = manifest else {
        return Ok(());
    };
    let next_references = manifest_references.checked_sub(reference_count).ok_or_else(|| {
        StorageError::Invariant(format!(
            "reference-search manifest for scope '{source_scope}' undercounts deleted path references"
        ))
    })?;
    let next_groups = manifest_groups.checked_sub(group_count).ok_or_else(|| {
        StorageError::Invariant(format!(
            "reference-search manifest for scope '{source_scope}' undercounts deleted path groups"
        ))
    })?;
    transaction.execute(
        "UPDATE code_repository_reference_search_manifests
         SET reference_count = ?2, group_count = ?3 WHERE source_scope = ?1",
        params![source_scope, next_references, next_groups],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite::code) fn count_code_rows(
    transaction: &rusqlite::Transaction<'_>,
    table: &'static str,
    source_scope: &str,
) -> Result<usize, StorageError> {
    transaction
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
