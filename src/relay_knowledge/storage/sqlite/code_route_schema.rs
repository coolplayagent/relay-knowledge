use rusqlite::Connection;

use crate::storage::StorageError;

use super::migrations::{code_schema_migration_applied, mark_code_schema_migration};

pub(in crate::storage::sqlite) const ROUTE_EXTRACTION_REINDEX_MIGRATION: &str =
    "web-route-extraction-reindex-v1";

pub(super) fn mark_legacy_route_extraction_scopes_stale_once(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, ROUTE_EXTRACTION_REINDEX_MIGRATION)? {
        return Ok(());
    }
    mark_all_code_scopes_with_files_stale(connection)?;
    mark_code_schema_migration(connection, ROUTE_EXTRACTION_REINDEX_MIGRATION)
}

fn mark_all_code_scopes_with_files_stale(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE code_repository_scopes
        SET stale = 1
        WHERE EXISTS (
            SELECT 1
            FROM code_repository_files file
            WHERE file.source_scope = code_repository_scopes.source_scope
        )
        ",
        [],
    )?;
    connection.execute(
        "
        UPDATE code_repositories
        SET stale = 1
        WHERE last_indexed_scope_id IN (
            SELECT source_scope
            FROM code_repository_scopes
            WHERE stale != 0
        )
        ",
        [],
    )?;

    Ok(())
}
