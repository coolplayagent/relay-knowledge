use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::StorageError;

use super::{
    import_compat,
    scope_tables::{CODE_SCOPE_TABLES, IMPORTED_DERIVED_SCOPE_TABLES},
    snapshot_import::{IMPORT_SCHEMA, copy_attached_code_table},
};

pub(in crate::storage::sqlite::code) fn import_repository_from_database(
    connection: &mut Connection,
    source_path: &Path,
    repository_id: &str,
    source_scope: Option<&str>,
) -> Result<(), StorageError> {
    connection.execute(
        &format!("ATTACH DATABASE ?1 AS {IMPORT_SCHEMA}"),
        params![source_path.display().to_string()],
    )?;
    let result = import_attached_repository(connection, repository_id, source_scope);
    let detach = connection.execute(&format!("DETACH DATABASE {IMPORT_SCHEMA}"), []);
    match (result, detach) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(StorageError::from(error)),
    }
}

fn import_attached_repository(
    connection: &mut Connection,
    repository_id: &str,
    source_scope: Option<&str>,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    import_repository_metadata(&transaction, repository_id)?;
    if let Some(source_scope) = source_scope {
        import_code_scope(&transaction, repository_id, source_scope)?;
    }
    transaction.commit()?;

    Ok(())
}

fn import_repository_metadata(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
) -> Result<(), StorageError> {
    let main_has_repository = transaction
        .query_row(
            "SELECT 1 FROM code_repositories WHERE repository_id = ?1",
            params![repository_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let copied = transaction.execute(
        &format!(
            "
            INSERT OR IGNORE INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                indexed_file_count, symbol_count, reference_count, chunk_count,
                stale, degraded_reason
            )
            SELECT repository_id, alias, root_path, path_filters_json, language_filters_json,
                   last_indexed_scope_id, last_indexed_commit, tree_hash, state,
                   indexed_file_count, symbol_count, reference_count, chunk_count,
                   stale, degraded_reason
            FROM {IMPORT_SCHEMA}.code_repositories
            WHERE repository_id = ?1
            "
        ),
        params![repository_id],
    )?;
    if !main_has_repository && copied == 0 {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository_id}' is missing from the import database"
        )));
    }
    transaction.execute(
        &format!(
            "
            INSERT OR IGNORE INTO code_repository_aliases (alias, repository_id)
            SELECT alias, repository_id
            FROM {IMPORT_SCHEMA}.code_repository_aliases
            WHERE repository_id = ?1
            "
        ),
        params![repository_id],
    )?;

    Ok(())
}

fn import_code_scope(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
) -> Result<(), StorageError> {
    if transaction
        .query_row(
            "SELECT 1 FROM code_repository_scopes WHERE source_scope = ?1",
            params![source_scope],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Ok(());
    }
    let imported_generated_detection_is_current =
        import_compat::attached_generated_detection_is_current(
            transaction,
            super::super::schema::GENERATED_DETECTION_REINDEX_MIGRATION,
        )?;

    super::super::cleanup::delete_scope_index(transaction, source_scope)?;
    let copied = transaction.execute(
        &format!(
            "
            INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, indexed_file_count,
                symbol_count, reference_count, chunk_count, stale, degraded_reason
            )
            SELECT source_scope, repository_id, resolved_commit_sha, tree_hash,
                   path_filters_json, language_filters_json, indexed_file_count,
                   symbol_count, reference_count, chunk_count, stale, degraded_reason
            FROM {IMPORT_SCHEMA}.code_repository_scopes
            WHERE source_scope = ?1 AND repository_id = ?2
            "
        ),
        params![source_scope, repository_id],
    )?;
    if copied == 0 {
        return Err(StorageError::InvalidInput(format!(
            "code repository '{repository_id}' has no importable source scope '{source_scope}'"
        )));
    }
    for table in CODE_SCOPE_TABLES {
        copy_attached_code_table(transaction, table, source_scope)?;
    }
    for table in IMPORTED_DERIVED_SCOPE_TABLES {
        copy_attached_code_table(transaction, table, source_scope)?;
    }
    super::super::generated::backfill_scope_path_generated_flags(transaction, source_scope)?;
    if !imported_generated_detection_is_current {
        super::super::generated::mark_scope_generated_detection_stale(transaction, source_scope)?;
    }
    super::super::search::backfill_search_metadata_for_scope(transaction, source_scope)?;

    Ok(())
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
