use rusqlite::{Connection, params};

use crate::storage::StorageError;

pub(in crate::storage::sqlite::software) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS software_entities (
            occurrence_id TEXT PRIMARY KEY,
            entity_key TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            entity_kind TEXT NOT NULL,
            name TEXT NOT NULL,
            namespace TEXT,
            source_kind TEXT NOT NULL,
            primary_evidence_path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            evidence_refs_json TEXT NOT NULL,
            attributes_json TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_entities_scope_kind
            ON software_entities(source_scope, entity_kind, name, occurrence_id);

        CREATE INDEX IF NOT EXISTS software_entities_stable_key
            ON software_entities(entity_key, source_scope, occurrence_id);

        CREATE TABLE IF NOT EXISTS software_statements (
            statement_id TEXT PRIMARY KEY,
            source_scope TEXT NOT NULL,
            subject_id TEXT NOT NULL,
            predicate TEXT NOT NULL,
            object_id TEXT,
            object_value TEXT,
            source_kind TEXT NOT NULL,
            evidence_refs_json TEXT NOT NULL,
            primary_evidence_path TEXT NOT NULL,
            assertion_mode TEXT NOT NULL,
            resolution_state TEXT NOT NULL,
            valid_from INTEGER,
            valid_to INTEGER,
            observed_at INTEGER,
            extractor_id TEXT NOT NULL,
            extractor_version TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            fact_state TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_statements_scope_order
            ON software_statements(
                source_scope, fact_state, predicate, primary_evidence_path,
                source_kind, statement_id
            );

        CREATE INDEX IF NOT EXISTS software_statements_subject
            ON software_statements(subject_id, predicate, source_scope);

        CREATE TABLE IF NOT EXISTS software_ontology_diagnostics (
            diagnostic_id TEXT PRIMARY KEY,
            source_scope TEXT NOT NULL,
            shape_id TEXT NOT NULL,
            code TEXT NOT NULL,
            severity TEXT NOT NULL,
            statement_id TEXT,
            entity_key TEXT,
            field TEXT NOT NULL,
            message TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_ontology_diagnostics_scope
            ON software_ontology_diagnostics(source_scope, severity, code, diagnostic_id);
        ",
    )?;
    Ok(())
}

pub(in crate::storage::sqlite::software) fn delete_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM software_ontology_diagnostics WHERE source_scope = ?1",
        params![source_scope],
    )?;
    connection.execute(
        "DELETE FROM software_statements WHERE source_scope = ?1",
        params![source_scope],
    )?;
    connection.execute(
        "DELETE FROM software_entities WHERE source_scope = ?1",
        params![source_scope],
    )?;
    Ok(())
}
