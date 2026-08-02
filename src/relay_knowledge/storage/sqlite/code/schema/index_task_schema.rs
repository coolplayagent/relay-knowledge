use rusqlite::Connection;

use crate::storage::StorageError;

#[cfg(test)]
#[path = "index_task_schema_tests.rs"]
mod tests;

pub(super) fn initialize_index_task_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS code_repository_index_checkpoints (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            state TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL,
            tree_hash TEXT NOT NULL,
            path_filters_json TEXT NOT NULL,
            language_filters_json TEXT NOT NULL,
            total_path_count INTEGER NOT NULL,
            parsed_file_count INTEGER NOT NULL,
            committed_file_count INTEGER NOT NULL,
            committed_symbol_count INTEGER NOT NULL,
            committed_reference_count INTEGER NOT NULL,
            committed_chunk_count INTEGER NOT NULL,
            batch_count INTEGER NOT NULL,
            last_path TEXT,
            resource_budget_json TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            error_message TEXT,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_index_tasks (
            task_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            alias TEXT NOT NULL,
            ref_selector TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL,
            tree_hash TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            path_filters_json TEXT NOT NULL,
            language_filters_json TEXT NOT NULL,
            mode_json TEXT NOT NULL,
            state TEXT NOT NULL,
            lease_owner TEXT,
            lease_expires_at_ms INTEGER,
            attempt_count INTEGER NOT NULL,
            next_retry_at_ms INTEGER NOT NULL,
            input_fingerprint TEXT NOT NULL,
            resource_budget_json TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            last_error_kind TEXT,
            last_error_message TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE,
            UNIQUE (repository_id, input_fingerprint)
        );

        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_claimable
            ON code_repository_index_tasks(state, next_retry_at_ms, created_at_ms);
        CREATE INDEX IF NOT EXISTS code_repository_index_tasks_repository
            ON code_repository_index_tasks(repository_id, state, created_at_ms);

        ",
    )?;
    Ok(())
}
