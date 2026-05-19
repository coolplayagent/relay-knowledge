use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::{
    domain::{CodeRepositorySetRefreshTaskRecord, CodeRepositorySetRefreshTaskState},
    storage::{
        CodeRepositorySetRefreshTaskClaimRequest, CodeRepositorySetRefreshTaskCompletion,
        CodeRepositorySetRefreshTaskFailure, CodeRepositorySetRefreshTaskSeed, StorageError,
    },
};

use super::super::helpers::stable_id;

pub(super) fn queue_refresh_task(
    connection: &mut Connection,
    task: CodeRepositorySetRefreshTaskSeed,
) -> Result<CodeRepositorySetRefreshTaskRecord, StorageError> {
    if let Some(existing) = task_by_fingerprint(connection, &task.set_id, &task.input_fingerprint)?
        && existing.state.is_unfinished()
    {
        return Ok(existing);
    }
    let task_id = stable_id(
        "code-repository-set-refresh-task",
        &format!("{}:{}", task.set_id, task.input_fingerprint),
    );
    connection.execute(
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

    task_by_fingerprint(connection, &task.set_id, &task.input_fingerprint)?.ok_or_else(|| {
        StorageError::InvalidInput("repository set refresh task was not persisted".to_owned())
    })
}

pub(super) fn claim_refresh_task(
    connection: &mut Connection,
    request: CodeRepositorySetRefreshTaskClaimRequest,
) -> Result<Option<CodeRepositorySetRefreshTaskRecord>, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
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

pub(super) fn complete_refresh_task(
    connection: &mut Connection,
    request: CodeRepositorySetRefreshTaskCompletion,
) -> Result<CodeRepositorySetRefreshTaskRecord, StorageError> {
    let changed = connection.execute(
        "
        UPDATE code_repository_set_refresh_tasks
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
            "repository set refresh task lease is no longer active".to_owned(),
        ));
    }

    task_by_id(connection, &request.task_id)?.ok_or_else(|| {
        StorageError::InvalidInput("completed repository set refresh task is missing".to_owned())
    })
}

pub(super) fn fail_refresh_task(
    connection: &mut Connection,
    request: CodeRepositorySetRefreshTaskFailure,
) -> Result<CodeRepositorySetRefreshTaskRecord, StorageError> {
    let next_state = if request.attempt_count >= request.max_attempts {
        CodeRepositorySetRefreshTaskState::DeadLetter
    } else {
        CodeRepositorySetRefreshTaskState::Retrying
    };
    let changed = connection.execute(
        "
        UPDATE code_repository_set_refresh_tasks
        SET state = ?4,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?5,
            last_error_kind = ?6,
            last_error_message = ?7,
            updated_at_ms = ?8
        WHERE task_id = ?1 AND lease_owner = ?2 AND attempt_count = ?3
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

    task_by_id(connection, &request.task_id)?.ok_or_else(|| {
        StorageError::InvalidInput("failed repository set refresh task is missing".to_owned())
    })
}

fn task_by_fingerprint(
    connection: &mut Connection,
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
    connection: &mut Connection,
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
