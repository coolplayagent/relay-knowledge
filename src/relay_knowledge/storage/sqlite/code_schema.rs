use rusqlite::Connection;

use crate::storage::StorageError;

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
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
            confidence_basis_points INTEGER NOT NULL DEFAULT 5000,
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

        CREATE VIRTUAL TABLE IF NOT EXISTS code_repository_search USING fts5(
            source_scope UNINDEXED,
            document_kind UNINDEXED,
            record_id UNINDEXED,
            path UNINDEXED,
            language_id UNINDEXED,
            content
        );

        CREATE INDEX IF NOT EXISTS code_repository_symbols_lookup
            ON code_repository_symbols(source_scope, name, qualified_name, path);
        CREATE INDEX IF NOT EXISTS code_repository_references_lookup
            ON code_repository_references(source_scope, name, kind, path);
        CREATE INDEX IF NOT EXISTS code_repository_calls_lookup
            ON code_repository_calls(source_scope, callee_name, caller_name, path);
        CREATE INDEX IF NOT EXISTS code_repository_imports_lookup
            ON code_repository_imports(source_scope, module, path);
        CREATE INDEX IF NOT EXISTS code_repository_chunks_lookup
            ON code_repository_chunks(source_scope, path);
        CREATE INDEX IF NOT EXISTS code_repository_scopes_lookup
            ON code_repository_scopes(repository_id, resolved_commit_sha, path_filters_json, language_filters_json);
        ",
    )?;
    backfill_code_repository_aliases(connection)?;
    backfill_code_repository_search(connection)?;

    Ok(())
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
    if !code_repository_search_is_empty(connection)? {
        return Ok(());
    }
    backfill_search_symbols(connection)?;
    backfill_search_references(connection)?;
    backfill_search_imports(connection)?;
    backfill_search_calls(connection)?;
    backfill_search_chunks(connection)?;

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
        SELECT source_scope, 'reference', reference_id, path, '',
               name || ' ' || kind || ' ' || coalesce(target_hint, '')
        FROM code_repository_references
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
        SELECT source_scope, 'import', import_id, path, '',
               module || ' ' || coalesce(target_hint, '')
        FROM code_repository_imports
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
    connection.execute(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        SELECT source_scope, 'call', call_id, path, '',
               coalesce(caller_name, '') || ' ' || callee_name || ' ' || coalesce(target_hint, '')
        FROM code_repository_calls
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
