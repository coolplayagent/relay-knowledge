//! SQLite DDL for persisted code-graph facts.

use rusqlite::Connection;

use crate::storage::StorageError;

pub(in crate::storage::sqlite) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        DROP INDEX IF EXISTS code_symbols_lookup;
        DROP INDEX IF EXISTS code_references_lookup;
        DROP INDEX IF EXISTS code_chunks_lookup;

        CREATE TABLE IF NOT EXISTS code_files (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            language_id TEXT NOT NULL,
            parse_status TEXT NOT NULL,
            diagnostic TEXT,
            created_graph_version INTEGER NOT NULL,
            PRIMARY KEY (source_scope, path)
        );

        CREATE TABLE IF NOT EXISTS code_symbols (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            symbol_id TEXT NOT NULL,
            name TEXT NOT NULL,
            kind TEXT NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            grammar_version TEXT NOT NULL,
            query_name TEXT NOT NULL,
            query_version TEXT NOT NULL,
            node_kind TEXT NOT NULL,
            capture_kind TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL,
            PRIMARY KEY (source_scope, path, symbol_id),
            FOREIGN KEY (source_scope, path)
                REFERENCES code_files(source_scope, path)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_references (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            reference_id TEXT NOT NULL,
            symbol_text TEXT NOT NULL,
            kind TEXT NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            resolution_state TEXT NOT NULL,
            target_symbol_id TEXT,
            grammar_version TEXT NOT NULL,
            query_name TEXT NOT NULL,
            query_version TEXT NOT NULL,
            node_kind TEXT NOT NULL,
            capture_kind TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL,
            PRIMARY KEY (source_scope, path, reference_id),
            FOREIGN KEY (source_scope, path)
                REFERENCES code_files(source_scope, path)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_chunks (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            chunk_id TEXT NOT NULL,
            content TEXT NOT NULL,
            start_byte INTEGER NOT NULL,
            end_byte INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            grammar_version TEXT,
            query_name TEXT,
            query_version TEXT,
            node_kind TEXT,
            capture_kind TEXT,
            created_graph_version INTEGER NOT NULL,
            PRIMARY KEY (source_scope, path, chunk_id),
            FOREIGN KEY (source_scope, path)
                REFERENCES code_files(source_scope, path)
                ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS code_chunk_symbols (
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            chunk_id TEXT NOT NULL,
            symbol_id TEXT NOT NULL,
            PRIMARY KEY (source_scope, path, chunk_id, symbol_id),
            FOREIGN KEY (source_scope, path, chunk_id)
                REFERENCES code_chunks(source_scope, path, chunk_id)
                ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS code_symbols_lookup
            ON code_symbols(source_scope, name, path);
        CREATE INDEX IF NOT EXISTS code_references_lookup
            ON code_references(source_scope, symbol_text, target_symbol_id);
        CREATE INDEX IF NOT EXISTS code_chunks_lookup
            ON code_chunks(source_scope, path);
        ",
    )?;

    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod schema_tests;
