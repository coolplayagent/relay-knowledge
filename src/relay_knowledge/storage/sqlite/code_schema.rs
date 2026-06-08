use rusqlite::Connection;

use crate::storage::StorageError;

const CALL_SEARCH_SIGNATURE_MIGRATION: &str = "call-search-symbol-signatures-v1";
const EDGE_SEARCH_LANGUAGE_ID_MIGRATION: &str = "edge-search-language-ids-v1";
pub(super) const GENERATED_DETECTION_REINDEX_MIGRATION: &str = "generated-detection-reindex-v1";
const SEARCH_BACKFILL_MIGRATION: &str = "code-search-backfill-v1";
const SEARCH_METADATA_BACKFILL_MIGRATION: &str = "code-search-metadata-backfill-v1";

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
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
            FOREIGN KEY (repository_id) REFERENCES code_repositories(repository_id) ON DELETE CASCADE
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
    super::code_generated::backfill_all_path_generated_flags(connection)?;
    mark_legacy_generated_detection_scopes_stale_once(connection)?;
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

fn backfill_code_repository_search(connection: &Connection) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, SEARCH_BACKFILL_MIGRATION)? {
        return Ok(());
    }
    if !code_repository_search_is_empty(connection)? {
        mark_code_schema_migration(connection, SEARCH_BACKFILL_MIGRATION)?;
        return Ok(());
    }
    backfill_search_symbols(connection)?;
    backfill_search_references(connection)?;
    backfill_search_imports(connection)?;
    backfill_search_dependencies(connection)?;
    backfill_search_feature_flags(connection)?;
    backfill_search_calls(connection)?;
    backfill_search_routes(connection)?;
    backfill_search_chunks(connection)?;
    mark_code_schema_migration(connection, SEARCH_BACKFILL_MIGRATION)?;

    Ok(())
}

fn backfill_code_repository_search_metadata(connection: &Connection) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, SEARCH_METADATA_BACKFILL_MIGRATION)? {
        return Ok(());
    }
    sync_code_repository_search_metadata(connection)?;
    mark_code_schema_migration(connection, SEARCH_METADATA_BACKFILL_MIGRATION)
}

fn sync_code_repository_search_metadata(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR IGNORE INTO code_repository_search_metadata (
            source_scope, document_kind, record_id, path, search_rowid
        )
        SELECT source_scope, document_kind, record_id, path, rowid
        FROM code_repository_search
        ",
        [],
    )?;

    Ok(())
}

fn code_repository_search_is_empty(connection: &Connection) -> Result<bool, StorageError> {
    connection
        .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|count| count == 0)
        .map_err(StorageError::from)
}

fn backfill_search_symbols(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_symbols",
        &[
            "source_scope",
            "symbol_snapshot_id",
            "path",
            "language_id",
            "name",
            "qualified_name",
            "kind",
            "signature",
            "doc_comment",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'symbol', symbol_snapshot_id, path, language_id,
               name || ' ' || qualified_name || ' ' || kind || ' ' || signature || ' ' ||
               coalesce(doc_comment, '')
        FROM code_repository_symbols
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_references(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_references",
        &[
            "source_scope",
            "reference_id",
            "path",
            "name",
            "kind",
            "target_hint",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT reference.source_scope, 'reference', reference.reference_id, reference.path,
               coalesce(file.language_id, ''),
               reference.name || ' ' || reference.kind || ' ' || coalesce(reference.target_hint, '')
        FROM code_repository_references reference
        LEFT JOIN code_repository_files file
          ON file.source_scope = reference.source_scope
         AND file.path = reference.path
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_imports(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_imports",
        &["source_scope", "import_id", "path", "module", "target_hint"],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT import.source_scope, 'import', import.import_id, import.path,
               coalesce(file.language_id, ''),
               import.module || ' ' || coalesce(import.target_hint, '')
        FROM code_repository_imports import
        LEFT JOIN code_repository_files file
          ON file.source_scope = import.source_scope
         AND file.path = import.path
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_dependencies(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_dependencies",
        &[
            "source_scope",
            "dependency_id",
            "path",
            "language_id",
            "ecosystem",
            "package_name",
            "requirement",
            "resolved_version",
            "dependency_group",
            "source_kind",
            "excerpt",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'dependency', dependency_id, path, language_id,
               ecosystem || ' ' || package_name || ' ' || coalesce(requirement, '') || ' ' ||
               coalesce(resolved_version, '') || ' ' || dependency_group || ' ' ||
               source_kind || ' ' || excerpt || ' ' || path
        FROM code_repository_dependencies
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_feature_flags(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_feature_flags",
        &[
            "source_scope",
            "usage_id",
            "path",
            "language_id",
            "name",
            "source_kind",
            "source_key",
            "edge_kind",
            "excerpt",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'feature_flag', usage_id, path, language_id,
               name || ' ' || source_kind || ' ' || source_key || ' ' || edge_kind || ' ' ||
               excerpt || ' ' || path
        FROM code_repository_feature_flags
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_calls(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_calls",
        &[
            "source_scope",
            "call_id",
            "path",
            "caller_name",
            "callee_name",
            "target_hint",
        ],
    )? {
        return Ok(());
    }
    insert_search_calls(connection)
}

fn backfill_search_routes(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_routes",
        &[
            "source_scope",
            "route_id",
            "path",
            "language_id",
            "url",
            "http_method",
            "handler_name",
            "framework",
        ],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'route', route_id, path, language_id,
               url || ' ' || http_method || ' ' || handler_name || ' ' || framework || ' ' || path
        FROM code_repository_routes
        ",
        [],
    )?;

    Ok(())
}

fn rebuild_call_search_documents_after_signature_upgrade(
    connection: &Connection,
) -> Result<(), StorageError> {
    if !call_search_supports_symbol_signatures(connection)?
        || code_schema_migration_applied(connection, CALL_SEARCH_SIGNATURE_MIGRATION)?
    {
        return Ok(());
    }

    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = rebuild_call_search_documents_with_migration_marker(connection);
    match result {
        Ok(()) => connection
            .execute_batch("COMMIT")
            .map_err(StorageError::from),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn rebuild_call_search_documents_with_migration_marker(
    connection: &Connection,
) -> Result<(), StorageError> {
    if code_schema_migration_applied(connection, CALL_SEARCH_SIGNATURE_MIGRATION)? {
        return Ok(());
    }
    sync_code_repository_search_metadata(connection)?;
    connection.execute(
        "
        DELETE FROM code_repository_search
        WHERE rowid IN (
            SELECT search_rowid
            FROM code_repository_search_metadata
            WHERE document_kind = 'call'
        )
        ",
        [],
    )?;
    connection.execute(
        "DELETE FROM code_repository_search_metadata WHERE document_kind = 'call'",
        [],
    )?;
    insert_search_calls(connection)?;
    sync_code_repository_search_metadata(connection)?;
    mark_code_schema_migration(connection, CALL_SEARCH_SIGNATURE_MIGRATION)
}

fn call_search_supports_symbol_signatures(connection: &Connection) -> Result<bool, StorageError> {
    Ok(table_has_columns(
        connection,
        "code_repository_calls",
        &[
            "source_scope",
            "call_id",
            "path",
            "caller_name",
            "callee_name",
            "target_hint",
            "caller_symbol_snapshot_id",
            "callee_symbol_snapshot_id",
        ],
    )? && table_has_columns(
        connection,
        "code_repository_symbols",
        &["source_scope", "symbol_snapshot_id", "signature"],
    )?)
}

fn insert_search_calls(connection: &Connection) -> Result<(), StorageError> {
    if !call_search_supports_symbol_signatures(connection)? {
        connection.execute(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            SELECT call.source_scope, 'call', call.call_id, call.path,
                   coalesce(file.language_id, ''),
                   coalesce(call.caller_name, '') || ' ' || call.callee_name || ' ' ||
                   coalesce(call.target_hint, '') || ' ' || call.path
            FROM code_repository_calls call
            LEFT JOIN code_repository_files file
              ON file.source_scope = call.source_scope
             AND file.path = call.path
            ",
            [],
        )?;

        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT call.source_scope, 'call', call.call_id, call.path,
               coalesce(file.language_id, ''),
               coalesce(call.caller_name, '') || ' ' || call.callee_name || ' ' ||
               coalesce(call.target_hint, '') || ' ' ||
               coalesce(caller.signature, '') || ' ' ||
               coalesce(callee.signature, '') || ' ' || call.path
        FROM code_repository_calls call
        LEFT JOIN code_repository_files file
          ON file.source_scope = call.source_scope
         AND file.path = call.path
        LEFT JOIN code_repository_symbols caller
          ON caller.source_scope = call.source_scope
         AND caller.symbol_snapshot_id = call.caller_symbol_snapshot_id
        LEFT JOIN code_repository_symbols callee
          ON callee.source_scope = call.source_scope
         AND callee.symbol_snapshot_id = call.callee_symbol_snapshot_id
        ",
        [],
    )?;

    Ok(())
}

fn backfill_search_chunks(connection: &Connection) -> Result<(), StorageError> {
    if !table_has_columns(
        connection,
        "code_repository_chunks",
        &["source_scope", "chunk_id", "path", "language_id", "content"],
    )? {
        return Ok(());
    }
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'chunk', chunk_id, path, language_id, content
        FROM code_repository_chunks
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
fn code_schema_migration_applied(
    connection: &Connection,
    name: &str,
) -> Result<bool, StorageError> {
    connection
        .query_row(
            "
            SELECT EXISTS (
                SELECT 1
                FROM code_repository_schema_migrations
                WHERE name = ?1
            )
            ",
            [name],
            |row| row.get::<_, bool>(0),
        )
        .map_err(StorageError::from)
}
fn mark_code_schema_migration(connection: &Connection, name: &str) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO code_repository_schema_migrations (name, applied_at_ms)
        VALUES (?1, CAST(strftime('%s', 'now') AS INTEGER) * 1000)
        ",
        [name],
    )?;
    Ok(())
}
fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}
fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    let columns = table_columns(connection, table)?;
    Ok(required_columns
        .iter()
        .all(|required| columns.iter().any(|column| column == required)))
}

#[cfg(test)]
#[path = "code_schema_tests.rs"]
mod tests;
