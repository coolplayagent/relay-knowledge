//! SQLite schema ownership and additive upgrades for local-file metadata.

use rusqlite::Connection;

use crate::storage::StorageError;

use super::content;

pub(in crate::storage::sqlite) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS file_index_roots (
            scope_id TEXT NOT NULL,
            root_id TEXT NOT NULL,
            root_path TEXT NOT NULL,
            indexed_file_count INTEGER NOT NULL DEFAULT 0,
            missing_file_count INTEGER NOT NULL DEFAULT 0,
            scan_error_count INTEGER NOT NULL DEFAULT 0,
            truncated INTEGER NOT NULL DEFAULT 0,
            content_truncated INTEGER NOT NULL DEFAULT 0,
            content_read_error_count INTEGER NOT NULL DEFAULT 0,
            indexed_content_count INTEGER NOT NULL DEFAULT 0,
            skipped_content_count INTEGER NOT NULL DEFAULT 0,
            unchanged_content_count INTEGER NOT NULL DEFAULT 0,
            stale_content_cursor_count INTEGER NOT NULL DEFAULT 0,
            last_indexed_at_ms INTEGER,
            last_error TEXT,
            PRIMARY KEY (scope_id, root_id)
        );

        CREATE TABLE IF NOT EXISTS file_index_entries (
            entry_key TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            root_id TEXT NOT NULL,
            path TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            file_name TEXT NOT NULL,
            extension TEXT,
            parent_dir TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            modified_at_ms INTEGER NOT NULL,
            fingerprint TEXT NOT NULL,
            status TEXT NOT NULL,
            last_error TEXT,
            indexed_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS file_index_entries_scope_root
            ON file_index_entries(scope_id, root_id, status);

        CREATE VIRTUAL TABLE IF NOT EXISTS file_index_search USING fts5(
            entry_key UNINDEXED,
            scope_id UNINDEXED,
            root_id UNINDEXED,
            path,
            relative_path,
            file_name,
            extension,
            parent_dir
        );
        ",
    )?;
    content::initialize_schema(connection)?;
    for (column, definition) in [
        ("content_truncated", "INTEGER NOT NULL DEFAULT 0"),
        ("content_read_error_count", "INTEGER NOT NULL DEFAULT 0"),
        ("indexed_content_count", "INTEGER NOT NULL DEFAULT 0"),
        ("skipped_content_count", "INTEGER NOT NULL DEFAULT 0"),
        ("unchanged_content_count", "INTEGER NOT NULL DEFAULT 0"),
        ("stale_content_cursor_count", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        super::super::schema::columns::ensure_column(
            connection,
            "file_index_roots",
            column,
            definition,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod mod_tests;
