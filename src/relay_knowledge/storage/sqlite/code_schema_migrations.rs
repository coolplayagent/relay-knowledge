use rusqlite::Connection;

use crate::storage::StorageError;

pub(super) fn code_schema_migration_applied(
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

pub(super) fn mark_code_schema_migration(
    connection: &Connection,
    name: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO code_repository_schema_migrations (name, applied_at_ms)
        VALUES (?1, CAST(strftime('%s', 'now') AS INTEGER) * 1000)
        ",
        [name],
    )?;
    Ok(())
}

pub(super) fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<bool, StorageError> {
    let columns = table_columns(connection, table)?;
    Ok(required_columns
        .iter()
        .all(|required| columns.iter().any(|column| column == required)))
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}
