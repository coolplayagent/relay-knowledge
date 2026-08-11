use std::{thread, time::Duration};

use rusqlite::Connection;

use crate::storage::StorageError;

const GRAPH_RETRIEVAL_SCHEMA_RETRY_DELAYS_MS: [u64; 4] = [10, 30, 90, 270];
pub(super) const GRAPH_BM25_REBUILD_TABLE: &str = "graph_bm25_rebuild";

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

pub(super) fn prepare_bm25_rebuild_table(connection: &Connection) -> Result<(), StorageError> {
    connection.execute("DROP TABLE IF EXISTS graph_bm25_retired", [])?;
    connection.execute("DROP TABLE IF EXISTS graph_bm25_rebuild", [])?;
    connection.execute_batch(GRAPH_BM25_REBUILD_SCHEMA_SQL)?;
    Ok(())
}

pub(super) fn activate_bm25_rebuild_table(connection: &Connection) -> Result<(), StorageError> {
    connection.execute("ALTER TABLE graph_bm25 RENAME TO graph_bm25_retired", [])?;
    connection.execute("ALTER TABLE graph_bm25_rebuild RENAME TO graph_bm25", [])?;
    Ok(())
}

pub(super) fn drop_retired_bm25_table(connection: &Connection) -> Result<(), StorageError> {
    connection.execute("DROP TABLE IF EXISTS graph_bm25_retired", [])?;
    Ok(())
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
        routing_key,
        source_scope,
        source_path,
        entity_labels,
        entity_aliases,
        content
    );

    CREATE TABLE IF NOT EXISTS graph_bm25_route_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        indexed_graph_version INTEGER NOT NULL,
        document_count INTEGER NOT NULL,
        state TEXT NOT NULL,
        algorithm_version TEXT NOT NULL,
        semantic_generation TEXT NOT NULL,
        vector_generation TEXT NOT NULL,
        rebuild_phase TEXT,
        rebuild_cursor TEXT,
        rebuild_semantic INTEGER,
        rebuild_vector INTEGER,
        rebuild_owner TEXT,
        rebuild_lease_expires_at_ms INTEGER
    );

    INSERT OR IGNORE INTO graph_bm25_route_state (
        id, indexed_graph_version, document_count, state, algorithm_version,
        semantic_generation, vector_generation,
        rebuild_phase, rebuild_cursor, rebuild_semantic, rebuild_vector,
        rebuild_owner, rebuild_lease_expires_at_ms
    ) VALUES (
        1, 0, 0, 'fresh',
        'simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4',
        'unknown', 'unknown', NULL, NULL, NULL, NULL, NULL, NULL
    );

    CREATE TABLE IF NOT EXISTS graph_bm25_route_documents (
        document_id TEXT PRIMARY KEY,
        fts_rowid INTEGER NOT NULL UNIQUE,
        document_kind TEXT NOT NULL,
        created_graph_version INTEGER NOT NULL,
        source_scope TEXT NOT NULL,
        source_path TEXT,
        label_gram_state TEXT NOT NULL,
        group_token TEXT NOT NULL,
        term_counts_json TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS graph_bm25_route_documents_scope_path
    ON graph_bm25_route_documents(source_scope, source_path, document_id);

    CREATE INDEX IF NOT EXISTS graph_bm25_route_documents_label_state
    ON graph_bm25_route_documents(
        label_gram_state, source_scope, created_graph_version, document_id
    );

    CREATE INDEX IF NOT EXISTS graph_bm25_route_documents_global_label_state
    ON graph_bm25_route_documents(
        label_gram_state, created_graph_version, document_id
    );

    CREATE TABLE IF NOT EXISTS graph_bm25_route_groups (
        source_scope TEXT NOT NULL,
        group_token TEXT NOT NULL,
        document_count INTEGER NOT NULL,
        PRIMARY KEY (source_scope, group_token)
    );

    CREATE TABLE IF NOT EXISTS graph_bm25_route_terms (
        term TEXT NOT NULL,
        source_scope TEXT NOT NULL,
        group_token TEXT NOT NULL,
        collection_frequency INTEGER NOT NULL,
        PRIMARY KEY (term, source_scope, group_token)
    );

    CREATE TABLE IF NOT EXISTS graph_bm25_route_term_totals (
        term TEXT PRIMARY KEY,
        document_frequency INTEGER NOT NULL
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
    CREATE INDEX IF NOT EXISTS graph_semantic_documents_version
    ON graph_semantic_documents(created_graph_version DESC, document_id);
    CREATE INDEX IF NOT EXISTS graph_vector_documents_scope_version
    ON graph_vector_documents(source_scope, created_graph_version DESC);
    ";

const GRAPH_BM25_REBUILD_SCHEMA_SQL: &str = "
    CREATE VIRTUAL TABLE graph_bm25_rebuild USING fts5(
        document_id UNINDEXED,
        document_kind UNINDEXED,
        evidence_id UNINDEXED,
        parent_evidence_id UNINDEXED,
        modality UNINDEXED,
        created_graph_version UNINDEXED,
        routing_key,
        source_scope,
        source_path,
        entity_labels,
        entity_aliases,
        content
    );
    ";

#[cfg(test)]
#[path = "schema_tests.rs"]
mod schema_tests;
