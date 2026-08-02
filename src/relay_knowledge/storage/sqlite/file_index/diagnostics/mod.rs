//! File-index root status reads and aggregate diagnostics projection.

use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::{FileIndexDiagnostics, FileIndexRootStatus, StorageError};

pub(in crate::storage::sqlite) fn diagnostics(
    connection: &Connection,
) -> Result<FileIndexDiagnostics, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT scope_id, root_id, root_path, indexed_file_count, missing_file_count,
               scan_error_count, truncated, content_truncated, content_read_error_count,
               indexed_content_count, skipped_content_count, unchanged_content_count,
               stale_content_cursor_count, last_indexed_at_ms, last_error
        FROM file_index_roots
        ORDER BY scope_id ASC, root_id ASC
        ",
    )?;
    let rows = statement.query_map([], map_root_status)?;
    let roots = rows.collect::<Result<Vec<_>, _>>()?;

    Ok(FileIndexDiagnostics {
        root_count: roots.len(),
        indexed_file_count: roots.iter().map(|root| root.indexed_file_count).sum(),
        missing_file_count: roots.iter().map(|root| root.missing_file_count).sum(),
        indexed_content_count: roots.iter().map(|root| root.indexed_content_count).sum(),
        skipped_content_count: roots.iter().map(|root| root.skipped_content_count).sum(),
        unchanged_content_count: roots.iter().map(|root| root.unchanged_content_count).sum(),
        stale_content_cursor_count: roots
            .iter()
            .map(|root| root.stale_content_cursor_count)
            .sum(),
        content_read_error_count: roots.iter().map(|root| root.content_read_error_count).sum(),
        scan_error_count: roots.iter().map(|root| root.scan_error_count).sum(),
        truncated_root_count: roots.iter().filter(|root| root.truncated).count(),
        roots,
        content_cursors: Vec::new(),
    })
}

pub(super) fn root_status(
    connection: &Connection,
    scope_id: &str,
    root_id: &str,
) -> Result<Option<FileIndexRootStatus>, StorageError> {
    connection
        .query_row(
            "
            SELECT scope_id, root_id, root_path, indexed_file_count, missing_file_count,
                   scan_error_count, truncated, content_truncated, content_read_error_count,
                   indexed_content_count, skipped_content_count, unchanged_content_count,
                   stale_content_cursor_count, last_indexed_at_ms, last_error
            FROM file_index_roots
            WHERE scope_id = ?1 AND root_id = ?2
            ",
            params![scope_id, root_id],
            map_root_status,
        )
        .optional()
        .map_err(StorageError::from)
}

fn map_root_status(row: &rusqlite::Row<'_>) -> Result<FileIndexRootStatus, rusqlite::Error> {
    Ok(FileIndexRootStatus {
        scope_id: row.get(0)?,
        root_id: row.get(1)?,
        root_path: row.get(2)?,
        indexed_file_count: row.get(3)?,
        missing_file_count: row.get(4)?,
        scan_error_count: row.get(5)?,
        truncated: row.get(6)?,
        content_truncated: row.get(7)?,
        content_read_error_count: row.get(8)?,
        indexed_content_count: row.get(9)?,
        skipped_content_count: row.get(10)?,
        unchanged_content_count: row.get(11)?,
        stale_content_cursor_count: row.get(12)?,
        last_indexed_at_ms: row.get(13)?,
        last_error: row.get(14)?,
    })
}

#[cfg(test)]
mod mod_tests;
