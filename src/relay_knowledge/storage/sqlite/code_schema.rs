use rusqlite::{Connection, params};

use crate::storage::StorageError;

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    migrate_legacy_code_scope_schema(connection)?;
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
    ensure_code_repository_compat_columns(connection)?;

    Ok(())
}

pub(super) fn ensure_code_repository_compat_columns(
    connection: &Connection,
) -> Result<(), StorageError> {
    add_column_if_missing(
        connection,
        "code_repository_symbols",
        "canonical_symbol_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_references",
        "target_hint",
        "TEXT",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_references",
        "resolution_state",
        "TEXT NOT NULL DEFAULT 'unresolved'",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_references",
        "confidence_basis_points",
        "INTEGER NOT NULL DEFAULT 5000",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_references",
        "confidence_tier",
        "TEXT NOT NULL DEFAULT 'ambiguous'",
    )?;
    add_column_if_missing(connection, "code_repository_imports", "target_hint", "TEXT")?;
    add_column_if_missing(
        connection,
        "code_repository_imports",
        "resolution_state",
        "TEXT NOT NULL DEFAULT 'unresolved'",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_imports",
        "confidence_basis_points",
        "INTEGER NOT NULL DEFAULT 10000",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_imports",
        "confidence_tier",
        "TEXT NOT NULL DEFAULT 'extracted'",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_calls",
        "callee_symbol_snapshot_id",
        "TEXT",
    )?;
    add_column_if_missing(connection, "code_repository_calls", "target_hint", "TEXT")?;
    add_column_if_missing(
        connection,
        "code_repository_calls",
        "resolution_state",
        "TEXT NOT NULL DEFAULT 'unresolved'",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_calls",
        "confidence_basis_points",
        "INTEGER NOT NULL DEFAULT 5000",
    )?;
    add_column_if_missing(
        connection,
        "code_repository_calls",
        "confidence_tier",
        "TEXT NOT NULL DEFAULT 'ambiguous'",
    )?;
    connection.execute(
        "
        UPDATE code_repository_symbols
        SET canonical_symbol_id = symbol_snapshot_id
        WHERE canonical_symbol_id = ''
        ",
        [],
    )?;

    Ok(())
}

fn migrate_legacy_code_scope_schema(connection: &Connection) -> Result<(), StorageError> {
    if !table_exists(connection, "code_repositories")? {
        return Ok(());
    }
    let repository_columns = table_columns(connection, "code_repositories")?;
    let files_columns = table_columns(connection, "code_repository_files")?;
    if repository_columns
        .iter()
        .any(|column| column == "last_indexed_scope_id")
        && files_columns.iter().any(|column| column == "source_scope")
    {
        return Ok(());
    }
    let suffix = legacy_suffix(connection)?;
    for table in [
        "code_repository_path_tombstones",
        "code_repository_scopes",
        "code_repository_file_diagnostics",
        "code_repository_chunks",
        "code_repository_calls",
        "code_repository_imports",
        "code_repository_references",
        "code_repository_symbols",
        "code_repository_files",
        "code_repositories",
    ] {
        if table_exists(connection, table)? {
            connection.execute(
                &format!("ALTER TABLE {table} RENAME TO {table}_legacy_{suffix}"),
                [],
            )?;
        }
    }

    Ok(())
}

fn legacy_suffix(connection: &Connection) -> Result<u64, StorageError> {
    let mut suffix = 1;
    loop {
        let name = format!("code_repositories_legacy_{suffix}");
        if !table_exists(connection, &name)? {
            return Ok(suffix);
        }
        suffix += 1;
    }
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            params![table],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists != 0)
        .map_err(StorageError::from)
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    if !columns.iter().any(|existing| existing == column) {
        connection.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }

    Ok(())
}
