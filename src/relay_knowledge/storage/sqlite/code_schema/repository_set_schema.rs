use rusqlite::Connection;

use crate::storage::StorageError;

#[cfg(test)]
#[path = "repository_set_schema_tests.rs"]
mod tests;

pub(super) fn initialize_repository_set_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS code_repository_sets (
            set_id TEXT PRIMARY KEY,
            alias TEXT NOT NULL UNIQUE,
            description TEXT,
            default_ref_policy_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS code_repository_set_members (
            set_id TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            repository_alias TEXT NOT NULL,
            ref_selector TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            path_filters_json TEXT NOT NULL,
            language_filters_json TEXT NOT NULL,
            priority INTEGER NOT NULL,
            PRIMARY KEY (set_id, repository_id, source_scope),
            FOREIGN KEY (set_id) REFERENCES code_repository_sets(set_id) ON DELETE CASCADE,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE,
            FOREIGN KEY (source_scope) REFERENCES code_repository_scopes(source_scope) ON DELETE RESTRICT
        );

        CREATE TABLE IF NOT EXISTS code_repository_cross_edges (
            edge_id TEXT PRIMARY KEY,
            set_id TEXT NOT NULL,
            from_source_scope TEXT NOT NULL,
            from_repository_id TEXT NOT NULL,
            from_record_kind TEXT NOT NULL,
            from_record_id TEXT NOT NULL,
            to_source_scope TEXT,
            to_repository_id TEXT,
            to_record_kind TEXT NOT NULL,
            to_record_id TEXT,
            edge_kind TEXT NOT NULL,
            resolution_state TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            confidence_tier TEXT NOT NULL,
            evidence_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            FOREIGN KEY (set_id) REFERENCES code_repository_sets(set_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_set_overlay_status (
            set_id TEXT PRIMARY KEY,
            state TEXT NOT NULL,
            refreshed_at_ms INTEGER,
            edge_count INTEGER NOT NULL,
            member_versions_json TEXT NOT NULL,
            degraded_reason TEXT,
            FOREIGN KEY (set_id) REFERENCES code_repository_sets(set_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_set_refresh_tasks (
            task_id TEXT PRIMARY KEY,
            set_id TEXT NOT NULL,
            set_alias TEXT NOT NULL,
            state TEXT NOT NULL,
            lease_owner TEXT,
            lease_expires_at_ms INTEGER,
            attempt_count INTEGER NOT NULL,
            next_retry_at_ms INTEGER NOT NULL,
            input_fingerprint TEXT NOT NULL,
            last_error_kind TEXT,
            last_error_message TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            FOREIGN KEY (set_id) REFERENCES code_repository_sets(set_id) ON DELETE CASCADE,
            UNIQUE (set_id, input_fingerprint)
        );

        CREATE INDEX IF NOT EXISTS code_repository_set_members_scope
            ON code_repository_set_members(source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_cross_edges_set_scope
            ON code_repository_cross_edges(set_id, from_source_scope, to_source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_set_refresh_tasks_claimable
            ON code_repository_set_refresh_tasks(state, next_retry_at_ms, created_at_ms);

        CREATE TABLE IF NOT EXISTS code_workspace_package_mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            set_id TEXT NOT NULL,
            package_name TEXT NOT NULL,
            ecosystem TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            workspace_format TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            UNIQUE (set_id, package_name, ecosystem)
        );

        CREATE INDEX IF NOT EXISTS code_workspace_package_mappings_set_package
            ON code_workspace_package_mappings(set_id, package_name, ecosystem);
        CREATE INDEX IF NOT EXISTS code_workspace_package_mappings_scope
            ON code_workspace_package_mappings(source_scope);

        ",
    )?;
    Ok(())
}
