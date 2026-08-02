//! Owns durable operational table initialization and compatible upgrades.

use rusqlite::Connection;

use crate::storage::StorageError;

pub(in crate::storage::sqlite) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS worker_tasks (
            task_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            evidence_id TEXT,
            target_graph_version INTEGER NOT NULL,
            state TEXT NOT NULL,
            lease_owner TEXT,
            lease_expires_at_ms INTEGER,
            attempt_count INTEGER NOT NULL,
            next_retry_at_ms INTEGER NOT NULL,
            input_fingerprint TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            last_error_kind TEXT,
            last_error_message TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            UNIQUE (kind, input_fingerprint)
        );

        CREATE INDEX IF NOT EXISTS worker_tasks_claimable
            ON worker_tasks(kind, state, next_retry_at_ms, created_at_ms);

        CREATE TABLE IF NOT EXISTS proposals (
            proposal_id TEXT PRIMARY KEY,
            source_scope TEXT NOT NULL,
            kind TEXT NOT NULL,
            state TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            origin TEXT NOT NULL,
            provenance_json TEXT NOT NULL DEFAULT '{}',
            confidence_basis_points INTEGER NOT NULL,
            decided_by TEXT,
            decision_reason TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS proposals_state_updated
            ON proposals(state, updated_at_ms DESC);

        CREATE TABLE IF NOT EXISTS proposal_conflicts (
            conflict_id TEXT PRIMARY KEY,
            proposal_id TEXT NOT NULL,
            existing_fact_kind TEXT NOT NULL,
            existing_fact_id TEXT NOT NULL,
            severity TEXT NOT NULL,
            reason TEXT NOT NULL,
            FOREIGN KEY (proposal_id) REFERENCES proposals(proposal_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS proposal_conflicts_by_proposal
            ON proposal_conflicts(proposal_id);

        CREATE TABLE IF NOT EXISTS audit_events (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            operation TEXT NOT NULL,
            interface TEXT NOT NULL,
            request_id TEXT NOT NULL,
            trace_id TEXT NOT NULL,
            status TEXT NOT NULL,
            actor TEXT,
            source_scope TEXT,
            graph_version INTEGER NOT NULL,
            detail_json TEXT NOT NULL,
            message TEXT,
            created_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS audit_events_operation_sequence
            ON audit_events(operation, sequence DESC);

        CREATE TABLE IF NOT EXISTS service_operator_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            state TEXT NOT NULL,
            silent_updates_enabled INTEGER NOT NULL,
            allowed_scopes_json TEXT NOT NULL,
            last_run_at_ms INTEGER,
            next_retry_at_ms INTEGER,
            last_error TEXT,
            updated_at_ms INTEGER NOT NULL
        );

        INSERT OR IGNORE INTO service_operator_state (
            id, state, silent_updates_enabled, allowed_scopes_json, updated_at_ms
        ) VALUES (1, 'disabled', 0, '[]', 0);
        ",
    )?;
    ensure_text_column(connection, "proposals", "provenance_json", "'{}'")?;

    Ok(())
}

fn ensure_text_column(
    connection: &Connection,
    table: &str,
    column: &str,
    default_sql: &str,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    let exists = columns
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == column);
    if exists {
        return Ok(());
    }

    connection.execute_batch(&format!(
        "ALTER TABLE {table} ADD COLUMN {column} TEXT NOT NULL DEFAULT {default_sql}"
    ))?;

    Ok(())
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
