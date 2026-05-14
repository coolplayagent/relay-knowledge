use std::collections::BTreeMap;

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

        CREATE VIRTUAL TABLE IF NOT EXISTS code_repository_search USING fts5(
            source_scope UNINDEXED,
            document_kind UNINDEXED,
            record_id UNINDEXED,
            path UNINDEXED,
            language_id UNINDEXED,
            content
        );

        CREATE TABLE IF NOT EXISTS code_repository_schema_migrations (
            name TEXT PRIMARY KEY
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
    backfill_code_repository_search(connection)?;

    Ok(())
}

pub(super) fn ensure_code_repository_compat_columns(
    connection: &Connection,
) -> Result<(), StorageError> {
    let mut should_rebuild_symbol_identities = add_column_if_missing(
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
    should_rebuild_symbol_identities |= !symbol_identity_v1_nested_migration_recorded(connection)?;
    should_rebuild_symbol_identities |= symbol_identities_have_direct_stale_ids(connection)?;
    if should_rebuild_symbol_identities {
        rebuild_code_repository_symbol_identities(connection)?;
    }

    Ok(())
}

fn rebuild_code_repository_symbol_identities(connection: &Connection) -> Result<(), StorageError> {
    let transaction = connection.unchecked_transaction()?;
    let mut statement = transaction.prepare(
        "
        SELECT source_scope, symbol_snapshot_id, repository_id, path, name, kind,
               line_start, line_end, qualified_name
        FROM code_repository_symbols
        ORDER BY source_scope ASC, path ASC, line_start ASC, line_end DESC, name ASC
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(SymbolIdentityRow {
            source_scope: row.get(0)?,
            symbol_snapshot_id: row.get(1)?,
            repository_id: row.get(2)?,
            path: row.get(3)?,
            name: row.get(4)?,
            kind: row.get(5)?,
            line_start: row.get(6)?,
            line_end: row.get(7)?,
            prefix: symbol_path_prefix(&row.get::<_, String>(8)?).to_owned(),
        })
    })?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let mut by_scope_path = BTreeMap::<(&str, &str), Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        by_scope_path
            .entry((row.source_scope.as_str(), row.path.as_str()))
            .or_default()
            .push(index);
    }
    let mut updates = Vec::new();
    for row_indices in by_scope_path.values_mut() {
        row_indices.sort_by(|left, right| {
            let left = &rows[*left];
            let right = &rows[*right];
            left.line_start
                .cmp(&right.line_start)
                .then_with(|| right.line_end.cmp(&left.line_end))
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut container_stack = Vec::<usize>::new();
        for row_index in row_indices {
            let row = &rows[*row_index];
            while container_stack
                .last()
                .is_some_and(|ancestor| rows[*ancestor].line_end < row.line_end)
            {
                container_stack.pop();
            }
            let mut segments = container_stack
                .iter()
                .map(|ancestor| rows[*ancestor].name.clone())
                .collect::<Vec<_>>();
            segments.push(row.name.clone());
            let qualified_name = format!("{}::{}", row.prefix, segments.join("."));
            let canonical_symbol_id = format!("repo://{}/{}", row.repository_id, qualified_name);
            updates.push((
                row.source_scope.clone(),
                row.symbol_snapshot_id.clone(),
                qualified_name,
                canonical_symbol_id,
            ));
            if symbol_container_kind(&row.kind) {
                container_stack.push(*row_index);
            }
        }
    }
    drop(statement);
    let mut update = transaction.prepare(
        "
        UPDATE code_repository_symbols
        SET qualified_name = ?3,
            canonical_symbol_id = ?4
        WHERE source_scope = ?1 AND symbol_snapshot_id = ?2
        ",
    )?;
    for (source_scope, symbol_snapshot_id, qualified_name, canonical_symbol_id) in updates {
        update.execute(params![
            source_scope,
            symbol_snapshot_id,
            qualified_name,
            canonical_symbol_id,
        ])?;
    }
    drop(update);
    transaction.execute(
        "
        INSERT OR IGNORE INTO code_repository_schema_migrations (name)
        VALUES ('symbol_identity_v1_nested')
        ",
        [],
    )?;
    transaction.commit()?;

    Ok(())
}

fn symbol_identity_v1_nested_migration_recorded(
    connection: &Connection,
) -> Result<bool, StorageError> {
    connection
        .query_row(
            "
            SELECT EXISTS(
                SELECT 1
                FROM code_repository_schema_migrations
                WHERE name = 'symbol_identity_v1_nested'
            )
            ",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists != 0)
        .map_err(StorageError::from)
}

fn symbol_identities_have_direct_stale_ids(connection: &Connection) -> Result<bool, StorageError> {
    connection
        .query_row(
            "
        SELECT EXISTS(
            SELECT 1
            FROM code_repository_symbols
            WHERE canonical_symbol_id = ''
               OR canonical_symbol_id = symbol_snapshot_id
               OR canonical_symbol_id NOT LIKE 'repo://%'
            LIMIT 1
        )
        ",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists != 0)
        .map_err(StorageError::from)
}

struct SymbolIdentityRow {
    source_scope: String,
    symbol_snapshot_id: String,
    repository_id: String,
    path: String,
    name: String,
    kind: String,
    line_start: u32,
    line_end: u32,
    prefix: String,
}

fn symbol_path_prefix(qualified_name: &str) -> &str {
    qualified_name
        .rsplit_once("::")
        .map_or(qualified_name, |(prefix, _)| prefix)
}

fn symbol_container_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "constructor" | "function" | "interface" | "method" | "module" | "type"
    )
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
    drop_renamed_legacy_lookup_indexes(connection)?;

    Ok(())
}

fn drop_renamed_legacy_lookup_indexes(connection: &Connection) -> Result<(), StorageError> {
    for index in [
        "code_repository_symbols_lookup",
        "code_repository_references_lookup",
        "code_repository_calls_lookup",
        "code_repository_imports_lookup",
        "code_repository_chunks_lookup",
        "code_repository_scopes_lookup",
    ] {
        connection.execute(&format!("DROP INDEX IF EXISTS {index}"), [])?;
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

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<bool, StorageError> {
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
        return Ok(true);
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_migration_backfills_canonical_symbol_ids() {
        let connection = Connection::open_in_memory().expect("connection should open");
        connection
            .execute_batch(
                "
                CREATE TABLE code_repositories (
                    repository_id TEXT PRIMARY KEY,
                    last_indexed_scope_id TEXT
                );
                CREATE TABLE code_repository_files (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    path TEXT NOT NULL,
                    PRIMARY KEY (source_scope, path)
                );
                CREATE TABLE code_repository_symbols (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    symbol_snapshot_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    qualified_name TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL,
                    PRIMARY KEY (source_scope, symbol_snapshot_id)
                );
                INSERT INTO code_repository_symbols (
                    repository_id, source_scope, symbol_snapshot_id, path, name, kind,
                    qualified_name, line_start, line_end
                )
                VALUES
                    ('repo', 'scope', 'outer-a', 'src/lib.rs', 'outer_a', 'function', 'src::lib.rs::outer_a', 1, 5),
                    ('repo', 'scope', 'inner-a', 'src/lib.rs', 'inner', 'function', 'src::lib.rs::inner', 2, 3),
                    ('repo', 'scope', 'outer-b', 'src/lib.rs', 'outer_b', 'function', 'src::lib.rs::outer_b', 6, 10),
                    ('repo', 'scope', 'inner-b', 'src/lib.rs', 'inner', 'function', 'src::lib.rs::inner', 7, 8);
                ",
            )
            .expect("legacy-compatible fixture should build");

        initialize_code_schema(&connection).expect("schema should initialize");

        let canonical_symbol_id = connection
            .query_row(
                "
                SELECT canonical_symbol_id
                FROM code_repository_symbols
                WHERE symbol_snapshot_id = 'inner-b'
                ",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("canonical id should load");
        assert_eq!(
            canonical_symbol_id,
            "repo://repo/src::lib.rs::outer_b.inner"
        );
        assert!(
            symbol_identity_v1_nested_migration_recorded(&connection)
                .expect("rebuilt identities should be recorded")
        );
        assert!(
            !symbol_identities_have_direct_stale_ids(&connection)
                .expect("rebuilt identities should not be stale")
        );
    }
}
