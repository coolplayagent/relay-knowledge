use rusqlite::Connection;

use crate::storage::StorageError;

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
