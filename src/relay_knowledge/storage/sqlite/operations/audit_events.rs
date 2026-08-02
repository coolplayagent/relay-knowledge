//! Durable audit-event writes, filtered reads, counting, and row decoding.

use rusqlite::{Connection, Row, params};

use crate::{
    domain::{AuditEventRecord, AuditStatus},
    storage::{AuditQueryRequest, NewAuditEvent, StorageError},
};

pub(in crate::storage::sqlite) fn insert_audit_event(
    connection: &Connection,
    event: NewAuditEvent,
) -> Result<AuditEventRecord, StorageError> {
    connection.execute(
        "
        INSERT INTO audit_events (
            operation, interface, request_id, trace_id, status, actor, source_scope,
            graph_version, detail_json, message, created_at_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ",
        params![
            event.operation,
            event.interface,
            event.request_id,
            event.trace_id,
            event.status.as_str(),
            event.actor,
            event.source_scope,
            event.graph_version,
            event.detail_json,
            event.message,
            event.now_ms,
        ],
    )?;
    let sequence = u64::try_from(connection.last_insert_rowid()).unwrap_or(u64::MAX);

    audit_event_by_sequence(connection, sequence)
}

pub(in crate::storage::sqlite) fn query_audit_events(
    connection: &Connection,
    request: AuditQueryRequest,
) -> Result<Vec<AuditEventRecord>, StorageError> {
    let limit = i64::try_from(request.limit.max(1)).unwrap_or(i64::MAX);
    if let Some(operation) = request.operation {
        let mut statement = connection.prepare(
            "
            SELECT sequence, operation, interface, request_id, trace_id, status, actor,
                   source_scope, graph_version, detail_json, message, created_at_ms
            FROM audit_events
            WHERE operation = ?1
            ORDER BY sequence DESC
            LIMIT ?2
            ",
        )?;
        let rows = statement.query_map(params![operation, limit], audit_event_from_row)?;
        return rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from);
    }

    let mut statement = connection.prepare(
        "
        SELECT sequence, operation, interface, request_id, trace_id, status, actor,
               source_scope, graph_version, detail_json, message, created_at_ms
        FROM audit_events
        ORDER BY sequence DESC
        LIMIT ?1
        ",
    )?;
    let rows = statement.query_map(params![limit], audit_event_from_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite) fn audit_event_count(
    connection: &Connection,
) -> Result<usize, StorageError> {
    let count = connection.query_row("SELECT COUNT(*) FROM audit_events", [], |row| {
        row.get::<_, u64>(0)
    })?;

    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

fn audit_event_by_sequence(
    connection: &Connection,
    sequence: u64,
) -> Result<AuditEventRecord, StorageError> {
    connection
        .query_row(
            "
            SELECT sequence, operation, interface, request_id, trace_id, status, actor,
                   source_scope, graph_version, detail_json, message, created_at_ms
            FROM audit_events
            WHERE sequence = ?1
            ",
            params![sequence],
            audit_event_from_row,
        )
        .map_err(StorageError::from)
}

fn audit_event_from_row(row: &Row<'_>) -> rusqlite::Result<AuditEventRecord> {
    Ok(AuditEventRecord {
        sequence: row.get(0)?,
        operation: row.get(1)?,
        interface: row.get(2)?,
        request_id: row.get(3)?,
        trace_id: row.get(4)?,
        status: parse_audit_status(row.get::<_, String>(5)?),
        actor: row.get(6)?,
        source_scope: row.get(7)?,
        graph_version: row.get(8)?,
        detail_json: row.get(9)?,
        message: row.get(10)?,
        created_at_ms: row.get(11)?,
    })
}

fn parse_audit_status(value: String) -> AuditStatus {
    AuditStatus::parse(&value).unwrap_or(AuditStatus::Failed)
}

#[cfg(test)]
#[path = "audit_events_tests.rs"]
mod tests;
