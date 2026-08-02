use rusqlite::{Connection, params};

use crate::{
    domain::GraphVersion,
    storage::{IndexRefreshFailure, IndexRefreshTask, IndexRefreshTaskState, StorageError},
};

use super::record::{inactive_lease_error, require_task};

pub(crate) fn fail_index_refresh_task(
    connection: &mut Connection,
    request: IndexRefreshFailure,
) -> Result<IndexRefreshTask, StorageError> {
    if request.max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "index refresh max attempts must be greater than zero".to_owned(),
        ));
    }
    let transaction = connection.transaction()?;
    let task = require_task(&transaction, &request.task_id)?;
    let next_state = if task.attempt_count >= request.max_attempts {
        IndexRefreshTaskState::DeadLetter
    } else {
        IndexRefreshTaskState::Retrying
    };
    let next_retry = request.now_ms.saturating_add(request.retry_backoff_ms);
    let updated = transaction.execute(
        "
        UPDATE index_refresh_tasks
        SET state = ?5,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?6,
            last_error_kind = ?7,
            last_error_message = ?8,
            updated_at_ms = ?9
        WHERE task_id = ?1
          AND state = 'running'
          AND lease_owner = ?2
          AND attempt_count = ?3
          AND lease_expires_at_ms > ?4
        ",
        params![
            &request.task_id,
            &request.lease_owner,
            request.attempt_count,
            request.now_ms,
            next_state.as_str(),
            next_retry,
            &request.error_kind,
            &request.error_message,
            request.now_ms
        ],
    )?;
    if updated != 1 {
        return Err(inactive_lease_error(&request.task_id));
    }
    transaction.execute(
        "
        UPDATE index_cursors
        SET state = 'failed', last_error = ?4
        WHERE kind = ?1 AND source_scope = ?2 AND modality = ?3
        ",
        params![
            task.kind.as_str(),
            &task.source_scope,
            task.modality.as_str(),
            &request.error_message
        ],
    )?;
    super::super::recompute_aggregate_status(&transaction, task.kind, GraphVersion::ZERO)?;
    transaction.commit()?;

    require_task(connection, &task.task_id)
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod failure_tests;
