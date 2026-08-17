use rusqlite::Connection;

use crate::storage::StorageError;

pub(super) fn initialize_retention_schema(connection: &Connection) -> Result<(), StorageError> {
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_scopes",
        "retiring",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS code_repository_scope_gc_jobs (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            phase TEXT NOT NULL,
            deleted_rows INTEGER NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            last_error TEXT,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_retention_jobs (
            repository_id TEXT PRIMARY KEY,
            initial_scope TEXT NOT NULL,
            cutoff_ms INTEGER NOT NULL,
            cutoff_publication_generation INTEGER NOT NULL DEFAULT 0,
            phase TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            last_error TEXT,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id)
                ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS code_repository_scope_gc_jobs_repository
            ON code_repository_scope_gc_jobs(repository_id, updated_at_ms, source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_retention_jobs_updated
            ON code_repository_retention_jobs(updated_at_ms, repository_id);
        CREATE INDEX IF NOT EXISTS code_repository_scopes_retention
            ON code_repository_scopes(repository_id, retiring, source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_set_members_repository_scope
            ON code_repository_set_members(repository_id, source_scope, set_id);
        CREATE INDEX IF NOT EXISTS code_repository_cross_edges_from_scope_gc
            ON code_repository_cross_edges(from_source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_cross_edges_to_scope_gc
            ON code_repository_cross_edges(to_source_scope);
        ",
    )?;
    super::super::super::schema::columns::ensure_column(
        connection,
        "code_repository_retention_jobs",
        "cutoff_publication_generation",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "retention_schema_tests.rs"]
mod tests;
