use std::{thread, time::Duration};

use rusqlite::Connection;

use crate::storage::StorageError;

const GRAPH_RETRIEVAL_SCHEMA_RETRY_DELAYS_MS: [u64; 4] = [10, 30, 90, 270];

pub(super) fn execute_retrieval_schema(connection: &Connection) -> Result<(), StorageError> {
    for delay_ms in GRAPH_RETRIEVAL_SCHEMA_RETRY_DELAYS_MS {
        match connection.execute_batch(RETRIEVAL_SCHEMA_SQL) {
            Ok(()) => return Ok(()),
            Err(error) if graph_retrieval_schema_error_is_retryable(&error) => {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(StorageError::from(error)),
        }
    }

    connection
        .execute_batch(RETRIEVAL_SCHEMA_SQL)
        .map_err(StorageError::from)
}

fn graph_retrieval_schema_error_is_retryable(error: &rusqlite::Error) -> bool {
    graph_retrieval_schema_error_message_is_retryable(&error.to_string())
}

fn graph_retrieval_schema_error_message_is_retryable(message: &str) -> bool {
    graph_bm25_transient_error_message(message)
}

pub(in crate::storage::sqlite::retrieval) fn graph_bm25_transient_error_message(
    message: &str,
) -> bool {
    message.contains("vtable constructor failed: graph_bm25")
        || message.contains("database schema is locked")
        || message.contains("database table is locked")
        || message.contains("database is locked")
}

const RETRIEVAL_SCHEMA_SQL: &str = "
    CREATE VIRTUAL TABLE IF NOT EXISTS graph_bm25 USING fts5(
        document_id UNINDEXED,
        document_kind UNINDEXED,
        evidence_id UNINDEXED,
        parent_evidence_id UNINDEXED,
        modality UNINDEXED,
        created_graph_version UNINDEXED,
        source_scope,
        source_path,
        entity_labels,
        entity_aliases,
        content
    );

    CREATE TABLE IF NOT EXISTS graph_semantic_documents (
        document_id TEXT PRIMARY KEY,
        document_kind TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        parent_evidence_id TEXT,
        modality TEXT NOT NULL,
        created_graph_version INTEGER NOT NULL,
        source_scope TEXT NOT NULL,
        source_path TEXT,
        entity_labels_json TEXT NOT NULL,
        content TEXT NOT NULL,
        token_signature_json TEXT NOT NULL,
        model TEXT NOT NULL,
        dimension INTEGER NOT NULL,
        source_hash TEXT NOT NULL,
        tokenizer_version TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS graph_vector_documents (
        document_id TEXT PRIMARY KEY,
        document_kind TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        parent_evidence_id TEXT,
        modality TEXT NOT NULL,
        created_graph_version INTEGER NOT NULL,
        source_scope TEXT NOT NULL,
        source_path TEXT,
        entity_labels_json TEXT NOT NULL,
        content TEXT NOT NULL,
        vector_json TEXT NOT NULL,
        model TEXT NOT NULL,
        dimension INTEGER NOT NULL,
        source_hash TEXT NOT NULL,
        tokenizer_version TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS graph_semantic_documents_scope_version
    ON graph_semantic_documents(source_scope, created_graph_version DESC);
    CREATE INDEX IF NOT EXISTS graph_vector_documents_scope_version
    ON graph_vector_documents(source_scope, created_graph_version DESC);
    ";

#[cfg(test)]
#[path = "schema_tests.rs"]
mod schema_tests;
