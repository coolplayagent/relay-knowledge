//! Reads bounded aggregate row counts for graph storage diagnostics.

use rusqlite::Connection;

use crate::storage::StorageError;

pub(super) fn count_rows(
    connection: &Connection,
    table: &'static str,
) -> Result<usize, StorageError> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let count = connection.query_row(&sql, [], |row| row.get::<_, usize>(0))?;

    Ok(count)
}
