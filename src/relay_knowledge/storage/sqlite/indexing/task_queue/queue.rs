use rusqlite::{Connection, TransactionBehavior, params};

use crate::storage::{
    IndexRefreshDiagnostics, IndexRefreshQueueRequest, IndexRefreshTask, IndexRefreshTaskState,
    StorageError,
};

use super::{
    planning::{PlannedTask, planned_tasks},
    record::{input_fingerprint, read_task, task_id},
};

pub(crate) fn queue_index_refreshes(
    connection: &mut Connection,
    request: IndexRefreshQueueRequest,
) -> Result<IndexRefreshDiagnostics, StorageError> {
    if request.max_queue_depth == 0 {
        return Err(StorageError::InvalidInput(
            "index refresh queue capacity must be greater than zero".to_owned(),
        ));
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let tasks = planned_tasks(&transaction, &request)?;
    let new_task_count = tasks
        .iter()
        .map(|task| task_id(task.kind, &task.source_scope, task.modality))
        .map(|id| match read_task(&transaction, &id) {
            Ok(task) => task.map_or(Ok(true), |task| {
                Ok(task_needs_enqueue(&task, request.reset_dead_letter_tasks))
            }),
            Err(error) => Err(error),
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|value| *value)
        .count();
    let current_depth = super::super::unfinished_task_count(&transaction)?;
    if current_depth.saturating_add(new_task_count) > request.max_queue_depth {
        return Err(StorageError::InvalidInput(format!(
            "index refresh queue capacity exceeded: depth={} new={} capacity={}",
            current_depth, new_task_count, request.max_queue_depth
        )));
    }

    for task in tasks {
        upsert_task(
            &transaction,
            task,
            request.now_ms,
            request.reset_dead_letter_tasks,
        )?;
    }

    let diagnostics = super::super::diagnostics(&transaction, request.now_ms)?;
    transaction.commit()?;

    Ok(diagnostics)
}

fn upsert_task(
    connection: &Connection,
    task: PlannedTask,
    now_ms: u64,
    reset_dead_letter_tasks: bool,
) -> Result<(), StorageError> {
    let task_id = task_id(task.kind, &task.source_scope, task.modality);
    let input_fingerprint = input_fingerprint(
        task.kind,
        &task.source_scope,
        task.modality,
        task.target_graph_version,
    );
    loop {
        let existing = read_task(connection, &task_id)?;
        match existing {
            None => return insert_task(connection, &task, &task_id, &input_fingerprint, now_ms),
            Some(existing) if existing.state == IndexRefreshTaskState::Succeeded => {
                if existing
                    .cursor_after
                    .is_some_and(|version| version >= task.target_graph_version)
                {
                    return Ok(());
                }
                if reset_task(
                    connection,
                    &task,
                    &task_id,
                    &input_fingerprint,
                    now_ms,
                    existing.state,
                )? {
                    return Ok(());
                }
            }
            Some(existing) if existing.state == IndexRefreshTaskState::DeadLetter => {
                if !reset_dead_letter_tasks {
                    return Ok(());
                }
                if reset_task(
                    connection,
                    &task,
                    &task_id,
                    &input_fingerprint,
                    now_ms,
                    existing.state,
                )? {
                    return Ok(());
                }
            }
            Some(existing) if existing.state == IndexRefreshTaskState::Running => return Ok(()),
            Some(existing) => {
                if extend_claimable_task(
                    connection,
                    &task,
                    &task_id,
                    &input_fingerprint,
                    now_ms,
                    &existing,
                )? {
                    return Ok(());
                }
            }
        }
    }
}

fn insert_task(
    connection: &Connection,
    task: &PlannedTask,
    task_id: &str,
    input_fingerprint: &str,
    now_ms: u64,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT INTO index_refresh_tasks (
            task_id, kind, source_scope, modality, target_graph_version, state,
            lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
            input_fingerprint, cursor_before, cursor_after, last_error_kind,
            last_error_message, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'queued', NULL, NULL, 0, ?6,
                ?7, ?8, NULL, NULL, NULL, ?9, ?9)
        ",
        params![
            task_id,
            task.kind.as_str(),
            task.source_scope,
            task.modality.as_str(),
            task.target_graph_version.get(),
            now_ms,
            input_fingerprint,
            task.cursor_before.get(),
            now_ms
        ],
    )?;

    Ok(())
}

fn reset_task(
    connection: &Connection,
    task: &PlannedTask,
    task_id: &str,
    input_fingerprint: &str,
    now_ms: u64,
    expected_state: IndexRefreshTaskState,
) -> Result<bool, StorageError> {
    let updated = connection.execute(
        "
        UPDATE index_refresh_tasks
        SET target_graph_version = ?2,
            state = 'queued',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            attempt_count = 0,
            next_retry_at_ms = ?3,
            input_fingerprint = ?4,
            cursor_before = ?5,
            cursor_after = NULL,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = ?6
        WHERE task_id = ?1
          AND state = ?7
        ",
        params![
            task_id,
            task.target_graph_version.get(),
            now_ms,
            input_fingerprint,
            task.cursor_before.get(),
            now_ms,
            expected_state.as_str()
        ],
    )?;

    Ok(updated == 1)
}

fn extend_claimable_task(
    connection: &Connection,
    task: &PlannedTask,
    task_id: &str,
    input_fingerprint: &str,
    now_ms: u64,
    existing: &IndexRefreshTask,
) -> Result<bool, StorageError> {
    let target = existing.target_graph_version.max(task.target_graph_version);
    let updated = connection.execute(
        "
        UPDATE index_refresh_tasks
        SET target_graph_version = ?2,
            input_fingerprint = ?3,
            cursor_before = MIN(cursor_before, ?4),
            updated_at_ms = ?5
        WHERE task_id = ?1
          AND state = ?6
        ",
        params![
            task_id,
            target.get(),
            input_fingerprint,
            task.cursor_before.get(),
            now_ms,
            existing.state.as_str()
        ],
    )?;

    Ok(updated == 1)
}

fn task_needs_enqueue(task: &IndexRefreshTask, reset_dead_letter_tasks: bool) -> bool {
    match task.state {
        IndexRefreshTaskState::Queued
        | IndexRefreshTaskState::Running
        | IndexRefreshTaskState::Retrying
        | IndexRefreshTaskState::Failed => false,
        IndexRefreshTaskState::DeadLetter => reset_dead_letter_tasks,
        IndexRefreshTaskState::Succeeded => true,
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod queue_tests;
