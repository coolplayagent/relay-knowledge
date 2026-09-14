use rusqlite::Connection;

use crate::storage::StorageError;

#[cfg(test)]
#[path = "repository_schema_tests.rs"]
mod tests;

pub(super) fn initialize_repository_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS code_repository_schema_migrations (
            name TEXT PRIMARY KEY,
            applied_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS code_repositories (
            repository_id TEXT PRIMARY KEY,
            alias TEXT NOT NULL UNIQUE,
            root_path TEXT NOT NULL,
            path_filters_json TEXT NOT NULL,
            language_filters_json TEXT NOT NULL,
            last_indexed_scope_id TEXT,
            last_indexed_commit TEXT,
            tree_hash TEXT,
            state TEXT NOT NULL,
            indexed_file_count INTEGER NOT NULL,
            symbol_count INTEGER NOT NULL,
            reference_count INTEGER NOT NULL,
            chunk_count INTEGER NOT NULL,
            stale INTEGER NOT NULL,
            degraded_reason TEXT
        );

        CREATE TABLE IF NOT EXISTS code_repository_aliases (
            alias TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_scopes (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL,
            tree_hash TEXT NOT NULL,
            path_filters_json TEXT NOT NULL,
            language_filters_json TEXT NOT NULL,
            indexed_file_count INTEGER NOT NULL,
            symbol_count INTEGER NOT NULL,
            reference_count INTEGER NOT NULL,
            chunk_count INTEGER NOT NULL,
            stale INTEGER NOT NULL,
            degraded_reason TEXT,
            retiring INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_commit_scopes (
            repository_id TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            published_sequence INTEGER NOT NULL,
            PRIMARY KEY (repository_id, resolved_commit_sha, source_scope),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE,
            FOREIGN KEY (source_scope) REFERENCES code_repository_scopes(source_scope) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS code_repository_commit_scopes_scope
            ON code_repository_commit_scopes(source_scope);
        CREATE INDEX IF NOT EXISTS code_repository_commit_scopes_retention
            ON code_repository_commit_scopes(
                repository_id, published_sequence DESC, resolved_commit_sha, source_scope
            );

        CREATE TABLE IF NOT EXISTS code_repository_files (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            blob_hash TEXT NOT NULL,
            byte_len INTEGER NOT NULL,
            line_count INTEGER NOT NULL,
            parse_status TEXT NOT NULL,
            is_generated INTEGER NOT NULL DEFAULT 0,
            degraded_reason TEXT,
            PRIMARY KEY (source_scope, path),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_symbols (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            symbol_snapshot_id TEXT NOT NULL,
            canonical_symbol_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            name TEXT NOT NULL,
            qualified_name TEXT NOT NULL,
            kind TEXT NOT NULL,
            signature TEXT NOT NULL,
            doc_comment TEXT,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            symbol_role_json TEXT,
            PRIMARY KEY (source_scope, symbol_snapshot_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_references (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            reference_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            target_symbol_snapshot_id TEXT,
            target_hint TEXT,
            resolution_state TEXT NOT NULL DEFAULT 'unresolved',
            confidence_basis_points INTEGER NOT NULL DEFAULT 2500,
            confidence_tier TEXT NOT NULL DEFAULT 'ambiguous',
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            PRIMARY KEY (source_scope, reference_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_imports (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            import_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            module TEXT NOT NULL,
            target_hint TEXT,
            resolution_state TEXT NOT NULL DEFAULT 'unresolved',
            confidence_basis_points INTEGER NOT NULL DEFAULT 10000,
            confidence_tier TEXT NOT NULL DEFAULT 'extracted',
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            PRIMARY KEY (source_scope, import_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_dependencies (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            dependency_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            ecosystem TEXT NOT NULL,
            package_name TEXT NOT NULL,
            requirement TEXT,
            resolved_version TEXT,
            dependency_group TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            is_lockfile INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            excerpt TEXT NOT NULL,
            PRIMARY KEY (source_scope, dependency_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_calls (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            call_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            caller_symbol_snapshot_id TEXT,
            caller_name TEXT,
            callee_symbol_snapshot_id TEXT,
            callee_name TEXT NOT NULL,
            target_hint TEXT,
            resolution_state TEXT NOT NULL DEFAULT 'unresolved',
            confidence_basis_points INTEGER NOT NULL DEFAULT 5000,
            confidence_tier TEXT NOT NULL DEFAULT 'ambiguous',
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            PRIMARY KEY (source_scope, call_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_feature_flags (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            feature_flag_id TEXT NOT NULL,
            usage_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            name TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            source_key TEXT NOT NULL,
            edge_kind TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            confidence_tier TEXT NOT NULL,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            excerpt TEXT NOT NULL,
            PRIMARY KEY (source_scope, usage_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_framework_nodes (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            node_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            framework TEXT NOT NULL,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            detail TEXT,
            symbol_snapshot_id TEXT,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            PRIMARY KEY (source_scope, node_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS code_repository_framework_nodes_lookup
            ON code_repository_framework_nodes(source_scope, framework, kind, name, path);
        CREATE INDEX IF NOT EXISTS code_repository_framework_nodes_target
            ON code_repository_framework_nodes(source_scope, framework, detail, path);

        CREATE TABLE IF NOT EXISTS code_repository_framework_edges (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            edge_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            framework TEXT NOT NULL,
            kind TEXT NOT NULL,
            source_node_id TEXT NOT NULL,
            target_node_id TEXT,
            target_hint TEXT,
            resolution_state TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            confidence_tier TEXT NOT NULL,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            PRIMARY KEY (source_scope, edge_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS code_repository_framework_edges_lookup
            ON code_repository_framework_edges(source_scope, framework, kind, target_hint, path);

        CREATE TABLE IF NOT EXISTS code_repository_routes (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            route_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            url TEXT NOT NULL,
            http_method TEXT NOT NULL,
            handler_name TEXT NOT NULL,
            handler_symbol_snapshot_id TEXT,
            framework TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            PRIMARY KEY (source_scope, route_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_chunks (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            chunk_id TEXT NOT NULL,
            file_id TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            content TEXT NOT NULL,
            byte_start INTEGER NOT NULL,
            byte_end INTEGER NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            symbol_snapshot_id TEXT,
            PRIMARY KEY (source_scope, chunk_id),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_file_diagnostics (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            parse_status TEXT NOT NULL,
            message TEXT NOT NULL,
            PRIMARY KEY (source_scope, path, message),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_repository_path_tombstones (
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            old_path TEXT NOT NULL,
            new_path TEXT,
            base_ref TEXT NOT NULL,
            head_ref TEXT NOT NULL,
            PRIMARY KEY (source_scope, old_path, base_ref, head_ref),
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
        );

        ",
    )?;
    Ok(())
}
