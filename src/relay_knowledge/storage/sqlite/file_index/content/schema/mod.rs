//! SQLite schema for local-file content, chunks, FTS, and freshness cursors.

use rusqlite::Connection;

use crate::storage::StorageError;

pub(in crate::storage::sqlite::file_index) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS file_content_entries (
            entry_key TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            root_id TEXT NOT NULL,
            path TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            indexed_at_ms INTEGER NOT NULL,
            graph_version INTEGER NOT NULL,
            status TEXT NOT NULL,
            skipped_reason TEXT
        );

        CREATE INDEX IF NOT EXISTS file_content_entries_scope_root
            ON file_content_entries(scope_id, root_id, status);

        CREATE TABLE IF NOT EXISTS file_content_chunks (
            chunk_id TEXT PRIMARY KEY,
            entry_key TEXT NOT NULL,
            chunk_index INTEGER NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            content TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS file_content_chunks_entry
            ON file_content_chunks(entry_key, chunk_index);

        CREATE VIRTUAL TABLE IF NOT EXISTS file_content_search USING fts5(
            chunk_id UNINDEXED,
            entry_key UNINDEXED,
            scope_id UNINDEXED,
            root_id UNINDEXED,
            path,
            relative_path,
            content
        );

        CREATE TABLE IF NOT EXISTS file_content_cursors (
            cursor_key TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            root_id TEXT NOT NULL,
            path TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            indexed_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            stale_reason TEXT,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS file_content_cursors_scope_root
            ON file_content_cursors(scope_id, root_id, state);
        ",
    )?;

    Ok(())
}

#[cfg(test)]
mod mod_tests;
