//! Owns lease-guarded code-index task completion and failure transitions.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    domain::{CodeIndexTaskRecord, CodeIndexTaskState},
    storage::{CodeIndexTaskCompletion, CodeIndexTaskFailure, StorageError},
};

use super::{
    lease::{inactive_lease_error, validate_lease_owner},
    record_mapping::{task_from_row, task_update_returning_sql},
};

pub(in crate::storage::sqlite::code) fn complete_task(
    connection: &mut Connection,
    request: CodeIndexTaskCompletion,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let lease_owner = validate_lease_owner(&request.lease_owner)?;
    let sql = task_update_returning_sql(
        "
        UPDATE code_repository_index_tasks
        SET state = 'succeeded',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = ?4
        WHERE task_id = ?1
          AND state = 'running'
          AND lease_owner = ?2
          AND attempt_count = ?3
          AND lease_expires_at_ms > ?4
        ",
    );
    let completed = super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let completed = transaction
            .query_row(
                &sql,
                params![
                    &request.task_id,
                    lease_owner,
                    request.attempt_count,
                    request.now_ms,
                ],
                task_from_row,
            )
            .optional()
            .map_err(StorageError::from)?;
        if let Some(task) = &completed {
            super::retention::prune_finished_task_history(
                &transaction,
                &task.repository_id,
                Some(&task.task_id),
            )?;
        }
        transaction.commit()?;
        Ok(completed)
    })?;

    completed.ok_or_else(|| inactive_lease_error(&request.task_id))
}

pub(in crate::storage::sqlite::code) fn fail_task(
    connection: &mut Connection,
    request: CodeIndexTaskFailure,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let lease_owner = validate_lease_owner(&request.lease_owner)?;
    if request.max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "code index task max attempts must be greater than zero".to_owned(),
        ));
    }
    let next_state = if request.attempt_count >= request.max_attempts {
        CodeIndexTaskState::DeadLetter
    } else {
        CodeIndexTaskState::Retrying
    };
    let sql = task_update_returning_sql(
        "
        UPDATE code_repository_index_tasks
        SET state = ?4,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?5,
            last_error_kind = ?6,
            last_error_message = ?7,
            updated_at_ms = ?8
        WHERE task_id = ?1
          AND state = 'running'
          AND lease_owner = ?2
          AND attempt_count = ?3
          AND lease_expires_at_ms > ?8
        ",
    );
    let failed = super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let failed = transaction
            .query_row(
                &sql,
                params![
                    &request.task_id,
                    lease_owner,
                    request.attempt_count,
                    next_state.as_str(),
                    request.now_ms.saturating_add(request.retry_backoff_ms),
                    &request.error_kind,
                    &request.error_message,
                    request.now_ms,
                ],
                task_from_row,
            )
            .optional()
            .map_err(StorageError::from)?;
        if let Some(task) = &failed {
            super::retention::prune_finished_task_history(
                &transaction,
                &task.repository_id,
                Some(&task.task_id),
            )?;
        }
        transaction.commit()?;
        Ok(failed)
    })?;

    failed.ok_or_else(|| inactive_lease_error(&request.task_id))
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;
