use rusqlite::Connection;

use crate::storage::StorageError;

mod index_task_schema;
mod migrations;
mod repository_schema;
mod repository_set_schema;
mod route_schema;
mod search_backfill;
mod search_schema;

use self::index_task_schema::initialize_index_task_schema;
use self::migrations::{
    code_schema_migration_applied, mark_code_schema_migration, table_has_columns,
};
use self::repository_schema::initialize_repository_schema;
use self::repository_set_schema::initialize_repository_set_schema;
#[cfg(test)]
pub(super) use self::route_schema::ROUTE_EXTRACTION_REINDEX_MIGRATION;
use self::route_schema::mark_legacy_route_extraction_scopes_stale_once;
use self::search_backfill::{
    backfill_code_repository_search, backfill_code_repository_search_metadata,
    rebuild_call_search_documents_after_signature_upgrade,
};
use self::search_schema::initialize_search_schema;

const CALL_SEARCH_SIGNATURE_MIGRATION: &str = "call-search-symbol-signatures-v1";
const EDGE_SEARCH_LANGUAGE_ID_MIGRATION: &str = "edge-search-language-ids-v1";
pub(super) const GENERATED_DETECTION_REINDEX_MIGRATION: &str = "generated-detection-reindex-v1";
const SEARCH_BACKFILL_MIGRATION: &str = "code-search-backfill-v1";
const SEARCH_METADATA_BACKFILL_MIGRATION: &str = "code-search-metadata-backfill-v1";

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    initialize_repository_schema(connection)?;
    initialize_index_task_schema(connection)?;
    initialize_repository_set_schema(connection)?;
    initialize_search_schema(connection)?;
    super::super::schema_columns::ensure_column(
        connection,
        "code_repository_schema_migrations",
        "applied_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "code_repository_files",
        "is_generated",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "code_repository_symbols",
        "symbol_role_json",
        "TEXT",
    )?;
    super::code_generated::backfill_all_path_generated_flags(connection)?;
    mark_legacy_generated_detection_scopes_stale_once(connection)?;
    mark_legacy_route_extraction_scopes_stale_once(connection)?;
    if table_has_columns(
        connection,
        "code_repository_calls",
        &["source_scope", "caller_name", "path", "line_start"],
    )? {
        connection.execute(
            "CREATE INDEX IF NOT EXISTS code_repository_calls_caller_lookup
             ON code_repository_calls(source_scope, caller_name, path, line_start)",
            [],
        )?;
    }
    backfill_code_repository_aliases(connection)?;
    backfill_code_repository_search(connection)?;
    backfill_code_repository_search_metadata(connection)?;
    rebuild_call_search_documents_after_signature_upgrade(connection)?;
    backfill_edge_search_language_ids(connection)?;

    Ok(())
}

fn mark_legacy_generated_detection_scopes_stale_once(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, GENERATED_DETECTION_REINDEX_MIGRATION)? {
        return Ok(());
    }
    super::code_generated::mark_all_generated_detection_scopes_stale(connection)?;
    mark_code_schema_migration(connection, GENERATED_DETECTION_REINDEX_MIGRATION)
}

fn backfill_code_repository_aliases(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR IGNORE INTO code_repository_aliases (alias, repository_id)
        SELECT alias, repository_id
        FROM code_repositories
        ",
        [],
    )?;

    Ok(())
}

fn backfill_edge_search_language_ids(connection: &Connection) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, EDGE_SEARCH_LANGUAGE_ID_MIGRATION)? {
        return Ok(());
    }
    connection.execute_batch("BEGIN IMMEDIATE")?;
    if let Err(error) = backfill_edge_search_language_ids_once(connection) {
        let _ = connection.execute_batch("ROLLBACK");
        return Err(error);
    }
    connection
        .execute_batch("COMMIT")
        .map_err(StorageError::from)
}
fn backfill_edge_search_language_ids_once(connection: &Connection) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, EDGE_SEARCH_LANGUAGE_ID_MIGRATION)? {
        return Ok(());
    }
    connection.execute(
        "
        UPDATE code_repository_search
        SET language_id = (
            SELECT file.language_id
            FROM code_repository_files file
            WHERE file.source_scope = code_repository_search.source_scope
              AND file.path = code_repository_search.path
            LIMIT 1
        )
        WHERE document_kind IN ('reference', 'import', 'call')
          AND language_id = ''
          AND EXISTS (
            SELECT 1
            FROM code_repository_files file
            WHERE file.source_scope = code_repository_search.source_scope
              AND file.path = code_repository_search.path
          )
        ",
        [],
    )?;
    mark_code_schema_migration(connection, EDGE_SEARCH_LANGUAGE_ID_MIGRATION)?;
    Ok(())
}
#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
