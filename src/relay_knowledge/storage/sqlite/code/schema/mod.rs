use rusqlite::Connection;

use crate::storage::StorageError;

mod index_task_schema;
mod migrations;
mod repository_schema;
mod repository_set_schema;
mod retention_activity_trigger_schema;
pub(in crate::storage::sqlite) mod retention_schema;
mod route_schema;
mod search_schema;

use self::index_task_schema::initialize_index_task_schema;
use self::migrations::{code_schema_migration_applied, mark_code_schema_migration};
use self::repository_schema::initialize_repository_schema;
use self::repository_set_schema::initialize_repository_set_schema;
use self::retention_schema::initialize_retention_schema;
#[cfg(test)]
pub(super) use self::route_schema::ROUTE_EXTRACTION_REINDEX_MIGRATION;
use self::route_schema::mark_legacy_route_extraction_scopes_stale_once;
#[cfg(test)]
use self::search_schema::ensure_search_query_indexes;
pub(in crate::storage::sqlite) use self::search_schema::validate_existing_query_indexes;
pub(in crate::storage::sqlite::code) use self::search_schema::{
    SearchQueryIndexAdvance, advance_search_query_index_repair, advance_search_query_indexes,
    prepare_query_indexes_for_empty_owners, prepare_restart_query_indexes,
    query_indexes_ready_for_fact_publication,
};
use self::search_schema::{initialize_search_schema, require_query_indexes_for_fact_publication};
use super::super::schema::marker::{
    REFERENCE_SEARCH_GROUP_V2_MIGRATION, SEARCH_OWNER_V2_MIGRATION,
};

pub(super) const GENERATED_DETECTION_REINDEX_MIGRATION: &str = "generated-detection-reindex-v1";
pub(super) const LOSSLESS_MARKDOWN_REINDEX_MIGRATION: &str =
    "lossless-markdown-source-windows-reindex-v1";
pub(super) const FRAMEWORK_GRAPH_REINDEX_MIGRATION: &str = "framework-graph-reindex-v1";
pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    let reference_search_owner_was_current =
        super::super::schema::marker::reference_search_group_schema_is_current(connection)?;
    initialize_repository_schema(connection)?;
    super::super::schema::columns::ensure_column(
        connection,
        "code_repository_schema_migrations",
        "applied_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    initialize_index_task_schema(connection)?;
    initialize_repository_set_schema(connection)?;
    initialize_search_schema(connection)?;
    initialize_retention_schema(connection)?;
    super::super::schema::columns::ensure_column(
        connection,
        "code_repository_files",
        "is_generated",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema::columns::ensure_column(
        connection,
        "code_repository_symbols",
        "symbol_role_json",
        "TEXT",
    )?;
    super::generated::backfill_all_path_generated_flags(connection)?;
    mark_legacy_generated_detection_scopes_stale_once(connection)?;
    mark_legacy_route_extraction_scopes_stale_once(connection)?;
    mark_legacy_markdown_scopes_stale_once(connection)?;
    mark_legacy_framework_graph_scopes_stale_once(connection)?;
    mark_legacy_search_owner_scopes_stale_once(connection)?;
    mark_legacy_reference_search_group_scopes_stale_once(
        connection,
        reference_search_owner_was_current,
    )?;
    backfill_code_repository_aliases(connection)?;
    validate_existing_query_indexes(connection)?;

    Ok(())
}

fn mark_legacy_framework_graph_scopes_stale_once(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, FRAMEWORK_GRAPH_REINDEX_MIGRATION)? {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute("UPDATE code_repository_scopes SET stale = 1", [])?;
    transaction.execute(
        "UPDATE code_repositories
         SET stale = 1
         WHERE last_indexed_scope_id IN (
             SELECT source_scope FROM code_repository_scopes WHERE stale != 0
         )",
        [],
    )?;
    mark_code_schema_migration(&transaction, FRAMEWORK_GRAPH_REINDEX_MIGRATION)?;
    transaction.commit().map_err(StorageError::from)
}

fn mark_legacy_markdown_scopes_stale_once(connection: &Connection) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, LOSSLESS_MARKDOWN_REINDEX_MIGRATION)? {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "
        UPDATE code_repository_scopes
        SET stale = 1
        WHERE EXISTS (
            SELECT 1
            FROM code_repository_files file
            WHERE file.source_scope = code_repository_scopes.source_scope
              AND file.language_id = 'markdown'
        )
        ",
        [],
    )?;
    transaction.execute(
        "
        UPDATE code_repositories
        SET stale = 1
        WHERE last_indexed_scope_id IN (
            SELECT scope.source_scope
            FROM code_repository_scopes scope
            WHERE EXISTS (
                SELECT 1
                FROM code_repository_files file
                WHERE file.source_scope = scope.source_scope
                  AND file.language_id = 'markdown'
            )
        )
        ",
        [],
    )?;
    mark_code_schema_migration(&transaction, LOSSLESS_MARKDOWN_REINDEX_MIGRATION)?;
    transaction.commit().map_err(StorageError::from)
}

fn mark_legacy_search_owner_scopes_stale_once(connection: &Connection) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, SEARCH_OWNER_V2_MIGRATION)? {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute("UPDATE code_repository_scopes SET stale = 1", [])?;
    transaction.execute(
        "UPDATE code_repositories
         SET stale = 1
         WHERE last_indexed_scope_id IN (
             SELECT source_scope FROM code_repository_scopes WHERE stale != 0
         )",
        [],
    )?;
    // This marker records only that the v2 writer and exact serving gate are installed. It does
    // not assert that legacy FTS rows have metadata owners; durable full indexing replaces them.
    mark_code_schema_migration(&transaction, SEARCH_OWNER_V2_MIGRATION)?;
    transaction.commit().map_err(StorageError::from)
}

fn mark_legacy_reference_search_group_scopes_stale_once(
    connection: &Connection,
    owner_schema_was_current: bool,
) -> Result<(), StorageError> {
    if owner_schema_was_current
        && code_schema_migration_applied(connection, REFERENCE_SEARCH_GROUP_V2_MIGRATION)?
    {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    transaction.execute("UPDATE code_repository_scopes SET stale = 1", [])?;
    transaction.execute(
        "UPDATE code_repositories
         SET stale = 1
         WHERE last_indexed_scope_id IN (
             SELECT source_scope FROM code_repository_scopes WHERE stale != 0
         )",
        [],
    )?;
    mark_code_schema_migration(&transaction, REFERENCE_SEARCH_GROUP_V2_MIGRATION)?;
    transaction.commit().map_err(StorageError::from)
}

#[cfg(test)]
pub(in crate::storage) fn ensure_code_query_indexes(
    connection: &Connection,
) -> Result<(), StorageError> {
    ensure_search_query_indexes(connection)
}

pub(in super::super) fn require_code_query_indexes_for_fact_publication(
    connection: &Connection,
) -> Result<(), StorageError> {
    require_query_indexes_for_fact_publication(connection)
}

fn mark_legacy_generated_detection_scopes_stale_once(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, GENERATED_DETECTION_REINDEX_MIGRATION)? {
        return Ok(());
    }
    super::generated::mark_all_generated_detection_scopes_stale(connection)?;
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
