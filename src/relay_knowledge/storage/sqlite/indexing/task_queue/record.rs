use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{GraphVersion, IndexKind, IndexModality},
    storage::{IndexRefreshTask, StorageError},
};

pub(super) fn task_id(kind: IndexKind, source_scope: &str, modality: IndexModality) -> String {
    let mut input = Vec::new();
    super::super::append_hash_part(&mut input, kind.as_str());
    super::super::append_hash_part(&mut input, source_scope);
    super::super::append_hash_part(&mut input, modality.as_str());

    format!("index-refresh:{:016x}", super::super::stable_hash64(&input))
}

pub(super) fn input_fingerprint(
    kind: IndexKind,
    source_scope: &str,
    modality: IndexModality,
    target_graph_version: GraphVersion,
) -> String {
    format!(
        "{}:{}:{}:{}",
        kind.as_str(),
        super::super::stable_hash64(source_scope.as_bytes()),
        modality.as_str(),
        target_graph_version.get()
    )
}

pub(super) fn read_task(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<IndexRefreshTask>, StorageError> {
    connection
        .query_row(
            "
            SELECT task_id, kind, source_scope, modality, target_graph_version,
                   state, lease_owner, lease_expires_at_ms, attempt_count,
                   next_retry_at_ms, input_fingerprint, cursor_before, cursor_after,
                   last_error_kind, last_error_message, created_at_ms, updated_at_ms
            FROM index_refresh_tasks
            WHERE task_id = ?1
            ",
            params![task_id],
            task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn require_task(
    connection: &Connection,
    task_id: &str,
) -> Result<IndexRefreshTask, StorageError> {
    read_task(connection, task_id)?.ok_or_else(|| {
        StorageError::InvalidInput(format!("index refresh task '{task_id}' is missing"))
    })
}

pub(super) fn inactive_lease_error(task_id: &str) -> StorageError {
    StorageError::InvalidInput(format!(
        "index refresh task '{task_id}' is not held by an active lease"
    ))
}

fn task_from_row(row: &rusqlite::Row<'_>) -> Result<IndexRefreshTask, rusqlite::Error> {
    let kind: String = row.get(1)?;
    let modality: String = row.get(3)?;
    let state: String = row.get(5)?;

    Ok(IndexRefreshTask {
        task_id: row.get(0)?,
        kind: super::super::parse_index_kind(&kind).map_err(super::super::invalid_to_sqlite)?,
        source_scope: row.get(2)?,
        modality: super::super::parse_index_modality(&modality)
            .map_err(super::super::invalid_to_sqlite)?,
        target_graph_version: GraphVersion::new(row.get(4)?),
        state: super::super::parse_task_state(&state).map_err(super::super::invalid_to_sqlite)?,
        lease_owner: row.get(6)?,
        lease_expires_at_ms: row.get(7)?,
        attempt_count: row.get(8)?,
        next_retry_at_ms: row.get(9)?,
        input_fingerprint: row.get(10)?,
        cursor_before: GraphVersion::new(row.get(11)?),
        cursor_after: row.get::<_, Option<u64>>(12)?.map(GraphVersion::new),
        last_error_kind: row.get(13)?,
        last_error_message: row.get(14)?,
        created_at_ms: row.get(15)?,
        updated_at_ms: row.get(16)?,
    })
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod record_tests;
