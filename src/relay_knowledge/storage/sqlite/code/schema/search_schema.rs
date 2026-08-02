use rusqlite::Connection;

use crate::storage::StorageError;

#[cfg(test)]
#[path = "search_schema_tests.rs"]
mod tests;

pub(super) fn initialize_search_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS code_repository_search USING fts5(
            source_scope UNINDEXED,
            document_kind UNINDEXED,
            record_id UNINDEXED,
            path UNINDEXED,
            language_id UNINDEXED,
            content
        );

        CREATE TABLE IF NOT EXISTS code_repository_search_metadata (
            source_scope TEXT NOT NULL,
            document_kind TEXT NOT NULL,
            record_id TEXT NOT NULL,
            path TEXT NOT NULL,
            search_rowid INTEGER NOT NULL UNIQUE,
            PRIMARY KEY (source_scope, document_kind, record_id)
        );
        CREATE INDEX IF NOT EXISTS code_repository_search_metadata_scope_kind
            ON code_repository_search_metadata(source_scope, document_kind);
        CREATE INDEX IF NOT EXISTS code_repository_search_metadata_scope_path
            ON code_repository_search_metadata(source_scope, path);

        CREATE INDEX IF NOT EXISTS code_repository_symbols_lookup
            ON code_repository_symbols(source_scope, name, qualified_name, path);
        CREATE INDEX IF NOT EXISTS code_repository_symbols_name_path_lookup
            ON code_repository_symbols(source_scope, name, path);
        CREATE INDEX IF NOT EXISTS code_repository_symbols_path_line_lookup
            ON code_repository_symbols(source_scope, path, line_end, line_start);
        CREATE INDEX IF NOT EXISTS code_repository_references_lookup
            ON code_repository_references(source_scope, name, kind, path);
        CREATE INDEX IF NOT EXISTS code_repository_calls_lookup
            ON code_repository_calls(source_scope, callee_name, caller_name, path);
        CREATE INDEX IF NOT EXISTS code_repository_feature_flags_lookup
            ON code_repository_feature_flags(source_scope, name, source_key, edge_kind, path);
        CREATE INDEX IF NOT EXISTS code_repository_routes_lookup
            ON code_repository_routes(source_scope, url, http_method, path);
        CREATE INDEX IF NOT EXISTS code_repository_routes_handler_lookup
            ON code_repository_routes(source_scope, handler_symbol_snapshot_id, path);
        CREATE INDEX IF NOT EXISTS code_repository_imports_lookup
            ON code_repository_imports(source_scope, module, path);
        CREATE INDEX IF NOT EXISTS code_repository_imports_target_lookup
            ON code_repository_imports(source_scope, target_hint, path);
        CREATE INDEX IF NOT EXISTS code_repository_dependencies_lookup
            ON code_repository_dependencies(source_scope, ecosystem, package_name, path);
        CREATE INDEX IF NOT EXISTS code_repository_dependencies_group_lookup
            ON code_repository_dependencies(source_scope, dependency_group, path);
        CREATE INDEX IF NOT EXISTS code_repository_chunks_lookup
            ON code_repository_chunks(source_scope, path);
        CREATE INDEX IF NOT EXISTS code_repository_chunks_symbol_lookup
            ON code_repository_chunks(source_scope, symbol_snapshot_id);
        CREATE INDEX IF NOT EXISTS code_repository_scopes_lookup
            ON code_repository_scopes(repository_id, resolved_commit_sha, path_filters_json, language_filters_json);
        ",
    )?;
    Ok(())
}
