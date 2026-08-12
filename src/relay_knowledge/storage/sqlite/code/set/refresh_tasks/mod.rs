//! Durable repository-set refresh task queue, leases, and completion transitions.

use rusqlite::{Connection, OptionalExtension, Row, Transaction, TransactionBehavior, params};

use crate::{
    domain::{CodeRepositorySetRefreshTaskRecord, CodeRepositorySetRefreshTaskState},
    storage::{
        CodeRepositorySetRefreshTaskClaimRequest, CodeRepositorySetRefreshTaskCompletion,
        CodeRepositorySetRefreshTaskFailure, CodeRepositorySetRefreshTaskSeed, StorageError,
    },
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod refresh_task_tests;

use super::super::super::evidence_identity::stable_id;

const MAX_UNFINISHED_REFRESH_TASKS_PER_SET: usize = 2;
const MAX_UNFINISHED_REFRESH_TASKS_GLOBAL: usize = 128;
const RETAIN_SUCCEEDED_REFRESH_TASKS_PER_SET: usize = 64;
const RETAIN_FAILURE_CLASS_REFRESH_TASKS_PER_STATE: usize = 32;
const REFRESH_TASK_AUDIT_PRUNE_BATCH: usize = 64;

pub(in crate::storage::sqlite::code) fn queue_refresh_task(
    connection: &mut Connection,
    task: CodeRepositorySetRefreshTaskSeed,
) -> Result<CodeRepositorySetRefreshTaskRecord, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(existing) =
        task_by_fingerprint(&transaction, &task.set_id, &task.input_fingerprint)?
        && existing.state.is_unfinished()
    {
        transaction.commit()?;
        return Ok(existing);
    }
    supersede_pending_refresh_tasks(&transaction, &task)?;
    enforce_refresh_task_capacity(&transaction, &task.set_id)?;
    let task_id = stable_id(
        "code-repository-set-refresh-task",
        &format!("{}:{}", task.set_id, task.input_fingerprint),
    );
    transaction.execute(
        "
        INSERT INTO code_repository_set_refresh_tasks (
            task_id, set_id, set_alias, state, attempt_count, next_retry_at_ms,
            input_fingerprint, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, 'queued', 0, ?4, ?5, ?4, ?4)
        ON CONFLICT(set_id, input_fingerprint) DO UPDATE SET
            set_alias = excluded.set_alias,
            state = 'queued',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            attempt_count = 0,
            next_retry_at_ms = excluded.next_retry_at_ms,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![
            task_id,
            task.set_id,
            task.set_alias,
            task.now_ms,
            task.input_fingerprint,
        ],
    )?;
    prune_terminal_refresh_task_history(&transaction, &task.set_id)?;
    let queued = task_by_fingerprint(&transaction, &task.set_id, &task.input_fingerprint)?
        .ok_or_else(|| {
            StorageError::InvalidInput("repository set refresh task was not persisted".to_owned())
        })?;
    transaction.commit()?;
    Ok(queued)
}

fn supersede_pending_refresh_tasks(
    transaction: &Transaction<'_>,
    task: &CodeRepositorySetRefreshTaskSeed,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE code_repository_set_refresh_tasks
         SET state = 'cancelled', lease_owner = NULL, lease_expires_at_ms = NULL,
             last_error_kind = 'superseded',
             last_error_message = 'superseded by a newer repository-set snapshot',
             updated_at_ms = ?3
         WHERE set_id = ?1 AND input_fingerprint <> ?2
           AND state IN ('queued', 'retrying')",
        params![task.set_id, task.input_fingerprint, task.now_ms],
    )?;
    Ok(())
}

fn enforce_refresh_task_capacity(
    transaction: &Transaction<'_>,
    set_id: &str,
) -> Result<(), StorageError> {
    let set_depth = unfinished_refresh_task_count(
        transaction,
        Some(set_id),
        MAX_UNFINISHED_REFRESH_TASKS_PER_SET,
    )?;
    if set_depth >= MAX_UNFINISHED_REFRESH_TASKS_PER_SET {
        return Err(StorageError::CapacityExceeded(format!(
            "repository-set refresh queue for '{set_id}' has {set_depth} unfinished tasks (capacity {MAX_UNFINISHED_REFRESH_TASKS_PER_SET}); retry after active work completes"
        )));
    }
    let global_depth =
        unfinished_refresh_task_count(transaction, None, MAX_UNFINISHED_REFRESH_TASKS_GLOBAL)?;
    if global_depth >= MAX_UNFINISHED_REFRESH_TASKS_GLOBAL {
        return Err(StorageError::CapacityExceeded(format!(
            "global repository-set refresh queue has {global_depth} unfinished tasks (capacity {MAX_UNFINISHED_REFRESH_TASKS_GLOBAL}); retry after active work completes"
        )));
    }
    Ok(())
}

fn unfinished_refresh_task_count(
    connection: &Connection,
    set_id: Option<&str>,
    limit: usize,
) -> Result<usize, StorageError> {
    match set_id {
        Some(set_id) => connection.query_row(
            "SELECT COUNT(*) FROM (
                 SELECT 1 FROM code_repository_set_refresh_tasks
                 WHERE state IN ('queued', 'running', 'retrying') AND set_id = ?1
                 LIMIT ?2
             )",
            params![set_id, limit],
            |row| row.get(0),
        ),
        None => connection.query_row(
            "SELECT COUNT(*) FROM (
                 SELECT 1 FROM code_repository_set_refresh_tasks
                 WHERE state IN ('queued', 'running', 'retrying')
                 LIMIT ?1
             )",
            params![limit],
            |row| row.get(0),
        ),
    }
    .map_err(StorageError::from)
}

fn prune_terminal_refresh_task_history(
    transaction: &Transaction<'_>,
    set_id: &str,
) -> Result<(), StorageError> {
    delete_refresh_task_history_after(
        transaction,
        set_id,
        "state = 'succeeded'",
        RETAIN_SUCCEEDED_REFRESH_TASKS_PER_SET,
    )?;
    for state_predicate in [
        "state = 'failed'",
        "state = 'dead_letter'",
        "state = 'cancelled'",
    ] {
        delete_refresh_task_history_after(
            transaction,
            set_id,
            state_predicate,
            RETAIN_FAILURE_CLASS_REFRESH_TASKS_PER_STATE,
        )?;
    }
    Ok(())
}

fn delete_refresh_task_history_after(
    transaction: &Transaction<'_>,
    set_id: &str,
    state_predicate: &'static str,
    retain: usize,
) -> Result<(), StorageError> {
    transaction.execute(
        &format!(
            "DELETE FROM code_repository_set_refresh_tasks WHERE task_id IN (
                 SELECT task_id FROM code_repository_set_refresh_tasks
                 WHERE set_id = ?1 AND {state_predicate}
                 ORDER BY updated_at_ms DESC, created_at_ms DESC, task_id DESC
                 LIMIT ?3 OFFSET ?2
             )"
        ),
        params![set_id, retain, REFRESH_TASK_AUDIT_PRUNE_BATCH],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite::code) fn claim_refresh_task(
    connection: &mut Connection,
    request: CodeRepositorySetRefreshTaskClaimRequest,
) -> Result<Option<CodeRepositorySetRefreshTaskRecord>, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "UPDATE code_repository_set_refresh_tasks
         SET state = 'dead_letter', lease_owner = NULL, lease_expires_at_ms = NULL,
             last_error_kind = 'lease_expired',
             last_error_message = 'repository-set refresh lease expired after the maximum attempts',
             updated_at_ms = ?1
         WHERE state = 'running' AND lease_expires_at_ms <= ?1
           AND attempt_count >= ?2",
        params![request.now_ms, request.max_attempts],
    )?;
    let task_id = if let Some(task_id) = request.task_id {
        transaction
            .query_row(
                "
                SELECT task_id
                FROM code_repository_set_refresh_tasks
                WHERE task_id = ?1
                  AND next_retry_at_ms <= ?2
                  AND attempt_count < ?3
                  AND (
                    state IN ('queued', 'retrying')
                    OR (state = 'running' AND lease_expires_at_ms <= ?2)
                  )
                  AND NOT EXISTS (
                      SELECT 1 FROM code_repository_set_refresh_tasks live
                      WHERE live.set_id = code_repository_set_refresh_tasks.set_id
                        AND live.task_id <> code_repository_set_refresh_tasks.task_id
                        AND live.state = 'running'
                        AND live.lease_expires_at_ms > ?2
                  )
                ",
                params![task_id, request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    } else {
        transaction
            .query_row(
                "
                SELECT task_id
                FROM code_repository_set_refresh_tasks
                WHERE next_retry_at_ms <= ?1
                  AND attempt_count < ?2
                  AND (
                    state IN ('queued', 'retrying')
                    OR (state = 'running' AND lease_expires_at_ms <= ?1)
                  )
                  AND NOT EXISTS (
                      SELECT 1 FROM code_repository_set_refresh_tasks live
                      WHERE live.set_id = code_repository_set_refresh_tasks.set_id
                        AND live.task_id <> code_repository_set_refresh_tasks.task_id
                        AND live.state = 'running'
                        AND live.lease_expires_at_ms > ?1
                  )
                ORDER BY created_at_ms ASC, task_id ASC
                LIMIT 1
                ",
                params![request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    };
    let Some(task_id) = task_id else {
        transaction.commit()?;
        return Ok(None);
    };
    let changed = transaction.execute(
        "
        UPDATE code_repository_set_refresh_tasks
        SET state = 'running',
            lease_owner = ?2,
            lease_expires_at_ms = ?3,
            attempt_count = attempt_count + 1,
            updated_at_ms = ?4
        WHERE task_id = ?1
          AND next_retry_at_ms <= ?4
          AND attempt_count < ?5
          AND (
            state IN ('queued', 'retrying')
            OR (state = 'running' AND lease_expires_at_ms <= ?4)
          )
          AND NOT EXISTS (
              SELECT 1 FROM code_repository_set_refresh_tasks live
              WHERE live.set_id = code_repository_set_refresh_tasks.set_id
                AND live.task_id <> code_repository_set_refresh_tasks.task_id
                AND live.state = 'running'
                AND live.lease_expires_at_ms > ?4
          )
        ",
        params![
            task_id,
            request.lease_owner,
            request.now_ms.saturating_add(request.lease_duration_ms),
            request.now_ms,
            request.max_attempts,
        ],
    )?;
    if changed == 0 {
        transaction.commit()?;
        return Ok(None);
    }
    let task = transaction.query_row(
        &task_select_sql("WHERE task_id = ?1"),
        params![task_id],
        task_from_row,
    )?;
    transaction.commit()?;

    Ok(Some(task))
}

pub(in crate::storage::sqlite::code) fn complete_refresh_task(
    connection: &mut Connection,
    request: CodeRepositorySetRefreshTaskCompletion,
) -> Result<CodeRepositorySetRefreshTaskRecord, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        "
        UPDATE code_repository_set_refresh_tasks
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
        params![
            request.task_id,
            request.lease_owner,
            request.attempt_count,
            request.now_ms,
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::InvalidInput(
            "repository set refresh task lease is no longer active".to_owned(),
        ));
    }
    let completed = task_by_id(&transaction, &request.task_id)?.ok_or_else(|| {
        StorageError::InvalidInput("completed repository set refresh task is missing".to_owned())
    })?;
    prune_terminal_refresh_task_history(&transaction, &completed.set_id)?;
    transaction.commit()?;
    Ok(completed)
}

pub(in crate::storage::sqlite::code) fn fail_refresh_task(
    connection: &mut Connection,
    request: CodeRepositorySetRefreshTaskFailure,
) -> Result<CodeRepositorySetRefreshTaskRecord, StorageError> {
    let next_state = if request.attempt_count >= request.max_attempts {
        CodeRepositorySetRefreshTaskState::DeadLetter
    } else {
        CodeRepositorySetRefreshTaskState::Retrying
    };
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        "
        UPDATE code_repository_set_refresh_tasks
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
        params![
            request.task_id,
            request.lease_owner,
            request.attempt_count,
            next_state.as_str(),
            request.now_ms.saturating_add(request.retry_backoff_ms),
            request.error_kind,
            request.error_message,
            request.now_ms,
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::InvalidInput(
            "repository set refresh task lease is no longer active".to_owned(),
        ));
    }
    let failed = task_by_id(&transaction, &request.task_id)?.ok_or_else(|| {
        StorageError::InvalidInput("failed repository set refresh task is missing".to_owned())
    })?;
    prune_terminal_refresh_task_history(&transaction, &failed.set_id)?;
    transaction.commit()?;
    Ok(failed)
}

fn task_by_fingerprint(
    connection: &Connection,
    set_id: &str,
    input_fingerprint: &str,
) -> Result<Option<CodeRepositorySetRefreshTaskRecord>, StorageError> {
    connection
        .query_row(
            &task_select_sql("WHERE set_id = ?1 AND input_fingerprint = ?2"),
            params![set_id, input_fingerprint],
            task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn task_by_id(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<CodeRepositorySetRefreshTaskRecord>, StorageError> {
    connection
        .query_row(
            &task_select_sql("WHERE task_id = ?1"),
            params![task_id],
            task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn task_select_sql(predicate: &str) -> String {
    format!(
        "
        SELECT task_id, set_id, set_alias, state, lease_owner, lease_expires_at_ms,
               attempt_count, next_retry_at_ms, input_fingerprint, last_error_kind,
               last_error_message, created_at_ms, updated_at_ms
        FROM code_repository_set_refresh_tasks
        {predicate}
        "
    )
}

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositorySetRefreshTaskRecord> {
    let state =
        CodeRepositorySetRefreshTaskState::parse(&row.get::<_, String>(3)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(CodeRepositorySetRefreshTaskRecord {
        task_id: row.get(0)?,
        set_id: row.get(1)?,
        set_alias: row.get(2)?,
        state,
        lease_owner: row.get(4)?,
        lease_expires_at_ms: row.get(5)?,
        attempt_count: row.get(6)?,
        next_retry_at_ms: row.get(7)?,
        input_fingerprint: row.get(8)?,
        last_error_kind: row.get(9)?,
        last_error_message: row.get(10)?,
        created_at_ms: row.get(11)?,
        updated_at_ms: row.get(12)?,
    })
}
