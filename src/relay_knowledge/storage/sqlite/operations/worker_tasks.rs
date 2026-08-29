//! Durable worker-task queue, lease transitions, status aggregation, and row decoding.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    domain::{
        GraphVersion, WorkerBackendState, WorkerKind, WorkerStatus, WorkerTaskRecord,
        WorkerTaskState,
    },
    identity::stable_hash64,
    storage::{
        StorageError, WorkerTaskClaimRequest, WorkerTaskCompletion, WorkerTaskFailure,
        WorkerTaskSeed,
    },
};

pub(in crate::storage::sqlite) fn queue_worker_tasks(
    connection: &Connection,
    tasks: Vec<WorkerTaskSeed>,
) -> Result<Vec<WorkerTaskRecord>, StorageError> {
    let mut records = Vec::with_capacity(tasks.len());
    for task in tasks {
        let task_id = worker_task_id(task.kind, &task.input_fingerprint);
        connection.execute(
            "
            INSERT OR IGNORE INTO worker_tasks (
                task_id, kind, source_scope, evidence_id, target_graph_version, state,
                attempt_count, next_retry_at_ms, input_fingerprint, payload_json,
                created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', 0, ?6, ?7, ?8, ?9, ?9)
            ",
            params![
                task_id,
                task.kind.as_str(),
                task.source_scope,
                task.evidence_id,
                task.target_graph_version.get(),
                task.now_ms,
                task.input_fingerprint,
                task.payload_json,
                task.now_ms,
            ],
        )?;
        let record = worker_task_by_kind_fingerprint(connection, task.kind, task_id.clone())?
            .ok_or_else(|| StorageError::InvalidInput("worker task was not queued".to_owned()))?;
        records.push(record);
    }

    Ok(records)
}

pub(in crate::storage::sqlite) fn worker_statuses(
    connection: &Connection,
) -> Result<Vec<WorkerStatus>, StorageError> {
    WorkerKind::ALL
        .into_iter()
        .map(|kind| worker_status(connection, kind))
        .collect()
}

pub(in crate::storage::sqlite) fn claim_worker_task(
    connection: &Connection,
    request: WorkerTaskClaimRequest,
) -> Result<Option<WorkerTaskRecord>, StorageError> {
    let kind_filter = request.kind.map(|kind| kind.as_str().to_owned());
    let row_id = if let Some(kind) = kind_filter.as_deref() {
        connection
            .query_row(
                "
                SELECT task_id
                FROM worker_tasks
                WHERE kind = ?1
                  AND next_retry_at_ms <= ?2
                  AND attempt_count < ?3
                  AND (
                    state IN ('queued', 'retrying')
                    OR (state = 'running' AND lease_expires_at_ms <= ?2)
                  )
                ORDER BY created_at_ms ASC
                LIMIT 1
                ",
                params![kind, request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    } else {
        connection
            .query_row(
                "
                SELECT task_id
                FROM worker_tasks
                WHERE next_retry_at_ms <= ?1
                  AND attempt_count < ?2
                  AND (
                    state IN ('queued', 'retrying')
                    OR (state = 'running' AND lease_expires_at_ms <= ?1)
                  )
                ORDER BY created_at_ms ASC
                LIMIT 1
                ",
                params![request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    };

    let Some(task_id) = row_id else {
        return Ok(None);
    };
    connection.execute(
        "
        UPDATE worker_tasks
        SET state = 'running',
            lease_owner = ?2,
            lease_expires_at_ms = ?3,
            attempt_count = attempt_count + 1,
            updated_at_ms = ?4
        WHERE task_id = ?1
        ",
        params![
            task_id,
            request.lease_owner,
            request.now_ms.saturating_add(request.lease_duration_ms),
            request.now_ms,
        ],
    )?;

    worker_task_by_id(connection, &task_id)
}

pub(in crate::storage::sqlite) fn complete_worker_task(
    connection: &Connection,
    request: WorkerTaskCompletion,
) -> Result<WorkerTaskRecord, StorageError> {
    let changed = connection.execute(
        "
        UPDATE worker_tasks
        SET state = 'succeeded',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = ?4
        WHERE task_id = ?1 AND lease_owner = ?2 AND attempt_count = ?3
        ",
        params![
            request.task_id,
            request.lease_owner,
            request.attempt_count,
            request.now_ms,
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::InvalidInput(
            "worker task completion did not match an active lease".to_owned(),
        ));
    }

    worker_task_by_id_required(connection, &request.task_id)
}

pub(in crate::storage::sqlite) fn fail_worker_task(
    connection: &Connection,
    request: WorkerTaskFailure,
) -> Result<WorkerTaskRecord, StorageError> {
    let state = if request.attempt_count >= request.max_attempts {
        WorkerTaskState::DeadLetter
    } else {
        WorkerTaskState::Retrying
    };
    let changed = connection.execute(
        "
        UPDATE worker_tasks
        SET state = ?4,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            last_error_kind = ?5,
            last_error_message = ?6,
            next_retry_at_ms = ?7,
            updated_at_ms = ?8
        WHERE task_id = ?1 AND lease_owner = ?2 AND attempt_count = ?3
        ",
        params![
            request.task_id,
            request.lease_owner,
            request.attempt_count,
            state.as_str(),
            request.error_kind,
            request.error_message,
            request.now_ms.saturating_add(request.retry_backoff_ms),
            request.now_ms,
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::InvalidInput(
            "worker task failure did not match an active lease".to_owned(),
        ));
    }

    worker_task_by_id_required(connection, &request.task_id)
}

fn worker_status(connection: &Connection, kind: WorkerKind) -> Result<WorkerStatus, StorageError> {
    let queue_depth = count_worker_state(connection, kind, "queued")?;
    let running_count = count_worker_state(connection, kind, "running")?;
    let retrying_count = count_worker_state(connection, kind, "retrying")?;
    let dead_letter_count = count_worker_state(connection, kind, "dead_letter")?;
    let last_error = connection
        .query_row(
            "
            SELECT last_error_message
            FROM worker_tasks
            WHERE kind = ?1 AND last_error_message IS NOT NULL
            ORDER BY updated_at_ms DESC
            LIMIT 1
            ",
            params![kind.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    Ok(WorkerStatus {
        kind,
        backend_state: WorkerBackendState::Fallback,
        endpoint_configured: false,
        queue_depth,
        running_count,
        retrying_count,
        dead_letter_count,
        last_error,
    })
}

fn count_worker_state(
    connection: &Connection,
    kind: WorkerKind,
    state: &str,
) -> Result<usize, StorageError> {
    let count = connection.query_row(
        "SELECT COUNT(*) FROM worker_tasks WHERE kind = ?1 AND state = ?2",
        params![kind.as_str(), state],
        |row| row.get::<_, u64>(0),
    )?;

    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

fn worker_task_by_kind_fingerprint(
    connection: &Connection,
    kind: WorkerKind,
    task_id: String,
) -> Result<Option<WorkerTaskRecord>, StorageError> {
    connection
        .query_row(
            "
            SELECT task_id, kind, source_scope, evidence_id, target_graph_version, state,
                   lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
                   input_fingerprint, payload_json, last_error_kind, last_error_message,
                   created_at_ms, updated_at_ms
            FROM worker_tasks
            WHERE kind = ?1 AND task_id = ?2
            ",
            params![kind.as_str(), task_id],
            worker_task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn worker_task_by_id(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<WorkerTaskRecord>, StorageError> {
    connection
        .query_row(
            "
            SELECT task_id, kind, source_scope, evidence_id, target_graph_version, state,
                   lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
                   input_fingerprint, payload_json, last_error_kind, last_error_message,
                   created_at_ms, updated_at_ms
            FROM worker_tasks
            WHERE task_id = ?1
            ",
            params![task_id],
            worker_task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn worker_task_by_id_required(
    connection: &Connection,
    task_id: &str,
) -> Result<WorkerTaskRecord, StorageError> {
    worker_task_by_id(connection, task_id)?
        .ok_or_else(|| StorageError::InvalidInput(format!("worker task '{task_id}' not found")))
}

fn worker_task_from_row(row: &Row<'_>) -> rusqlite::Result<WorkerTaskRecord> {
    let kind = parse_worker_kind(row.get::<_, String>(1)?);
    let state = parse_worker_task_state(row.get::<_, String>(5)?);
    Ok(WorkerTaskRecord {
        task_id: row.get(0)?,
        kind,
        source_scope: row.get(2)?,
        evidence_id: row.get(3)?,
        target_graph_version: GraphVersion::new(row.get::<_, u64>(4)?),
        state,
        lease_owner: row.get(6)?,
        lease_expires_at_ms: row.get(7)?,
        attempt_count: row.get(8)?,
        next_retry_at_ms: row.get(9)?,
        input_fingerprint: row.get(10)?,
        payload_json: row.get(11)?,
        last_error_kind: row.get(12)?,
        last_error_message: row.get(13)?,
        created_at_ms: row.get(14)?,
        updated_at_ms: row.get(15)?,
    })
}

fn parse_worker_kind(value: String) -> WorkerKind {
    WorkerKind::parse(&value).unwrap_or(WorkerKind::Extractor)
}

fn parse_worker_task_state(value: String) -> WorkerTaskState {
    WorkerTaskState::parse(&value).unwrap_or(WorkerTaskState::Failed)
}

fn worker_task_id(kind: WorkerKind, fingerprint: &str) -> String {
    format!(
        "worker:{}:{:016x}",
        kind.as_str(),
        stable_hash64(fingerprint.as_bytes())
    )
}

#[cfg(test)]
#[path = "worker_tasks_tests.rs"]
mod tests;
