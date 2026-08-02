use rusqlite::{Connection, params};

use crate::storage::StorageError;

use super::super::{
    code, code_graph, connection_runtime, file_index, indexing, operations, retrieval,
};
use super::{columns, marker};

pub(in crate::storage::sqlite) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection_runtime::retry::retry_sqlite_transient(|| initialize_schema_once(connection))
}

fn initialize_schema_once(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS graph_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            graph_version INTEGER NOT NULL
        );

        INSERT OR IGNORE INTO graph_state (id, graph_version) VALUES (1, 0);

        CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS evidence (
            id TEXT PRIMARY KEY,
            source_scope TEXT NOT NULL,
            source_path TEXT,
            span_start_byte INTEGER,
            span_end_byte INTEGER,
            span_start_line INTEGER,
            span_end_line INTEGER,
            content TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL DEFAULT 10000,
            status TEXT NOT NULL DEFAULT 'accepted',
            modality TEXT NOT NULL DEFAULT 'text_span',
            source_uri TEXT,
            source_hash TEXT,
            media_hash TEXT,
            extractor TEXT,
            extractor_version TEXT,
            observed_at TEXT,
            parent_evidence_id TEXT,
            layout_page_number INTEGER,
            layout_x INTEGER,
            layout_y INTEGER,
            layout_width INTEGER,
            layout_height INTEGER,
            embedding_model TEXT,
            embedding_dimension INTEGER,
            extraction_status TEXT NOT NULL DEFAULT 'succeeded',
            extraction_message TEXT,
            created_graph_version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS evidence_entities (
            evidence_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            PRIMARY KEY (evidence_id, entity_id),
            FOREIGN KEY (evidence_id) REFERENCES evidence(id) ON DELETE CASCADE,
            FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS graph_mutations (
            graph_version INTEGER PRIMARY KEY,
            evidence_count INTEGER NOT NULL,
            entity_count INTEGER NOT NULL,
            relation_count INTEGER NOT NULL DEFAULT 0,
            claim_count INTEGER NOT NULL DEFAULT 0,
            event_count INTEGER NOT NULL DEFAULT 0,
            affected_scopes_json TEXT NOT NULL DEFAULT '[]',
            affected_entity_ids_json TEXT NOT NULL DEFAULT '[]',
            evidence_ids_json TEXT NOT NULL DEFAULT '[]',
            source_hashes_json TEXT NOT NULL DEFAULT '[]'
        );

        CREATE TABLE IF NOT EXISTS graph_relations (
            id TEXT PRIMARY KEY,
            source_entity_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            target_entity_id TEXT NOT NULL,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL,
            FOREIGN KEY (source_entity_id) REFERENCES entities(id),
            FOREIGN KEY (target_entity_id) REFERENCES entities(id)
        );

        CREATE TABLE IF NOT EXISTS graph_claims (
            id TEXT PRIMARY KEY,
            subject_entity_id TEXT NOT NULL,
            predicate TEXT NOT NULL,
            object TEXT NOT NULL,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL,
            FOREIGN KEY (subject_entity_id) REFERENCES entities(id)
        );

        CREATE TABLE IF NOT EXISTS graph_events (
            id TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            occurred_at TEXT,
            evidence_ids_json TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            status TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            created_graph_version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS graph_event_entities (
            event_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            PRIMARY KEY (event_id, entity_id),
            FOREIGN KEY (event_id) REFERENCES graph_events(id) ON DELETE CASCADE,
            FOREIGN KEY (entity_id) REFERENCES entities(id)
        );

        CREATE TABLE IF NOT EXISTS graph_fact_evidence (
            fact_kind TEXT NOT NULL,
            fact_id TEXT NOT NULL,
            evidence_id TEXT NOT NULL,
            PRIMARY KEY (fact_kind, fact_id, evidence_id),
            FOREIGN KEY (evidence_id) REFERENCES evidence(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS graph_fact_evidence_by_evidence
            ON graph_fact_evidence(evidence_id, fact_kind);
        ",
    )?;
    columns::ensure_core_schema_columns(connection)?;
    code::initialize_code_schema(connection)?;
    indexing::initialize_schema(connection)?;
    code_graph::initialize_schema(connection)?;
    operations::initialize_schema(connection)?;
    file_index::initialize_schema(connection)?;
    connection_runtime::maintenance::initialize_schema(connection)?;
    backfill_fact_evidence_links(connection)?;
    retrieval::initialize_schema(connection)?;
    marker::initialize_schema_marker(connection)?;

    Ok(())
}

fn backfill_fact_evidence_links(connection: &Connection) -> Result<(), StorageError> {
    backfill_fact_evidence_kind(connection, "relation", "graph_relations")?;
    backfill_fact_evidence_kind(connection, "claim", "graph_claims")?;
    backfill_fact_evidence_kind(connection, "event", "graph_events")?;

    Ok(())
}

fn backfill_fact_evidence_kind(
    connection: &Connection,
    fact_kind: &'static str,
    table: &'static str,
) -> Result<(), StorageError> {
    let mut statement =
        connection.prepare(&format!("SELECT id, evidence_ids_json FROM {table}"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let facts = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    drop(statement);

    for (fact_id, evidence_json) in facts {
        let evidence_ids: Vec<String> = serde_json::from_str(&evidence_json)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        for evidence_id in evidence_ids {
            connection.execute(
                "
                INSERT OR IGNORE INTO graph_fact_evidence (fact_kind, fact_id, evidence_id)
                SELECT ?1, ?2, e.id
                FROM evidence e
                WHERE e.id = ?3
                ",
                params![fact_kind, fact_id, evidence_id],
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "initialization_tests.rs"]
mod tests;
