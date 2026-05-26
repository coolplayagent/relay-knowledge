use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::{
    domain::{
        CodeIndexCheckpoint, CodeIndexResourceBudget, CodeIndexTaskRecord, CodeIndexTaskState,
        CodeScopeRetentionSummary,
    },
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskFailure,
        CodeIndexTaskLeaseRenewal, CodeIndexTaskSeed, CodeScopeRetentionRequest, StorageError,
    },
};

use super::{code_cleanup::delete_scope_index, code_status::parse_json_list};

const TASK_RECORD_COLUMNS: &str = "
    task_id, repository_id, alias, ref_selector, resolved_commit_sha, tree_hash,
    source_scope, path_filters_json, language_filters_json, mode_json, state,
    lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
    input_fingerprint, resource_budget_json, payload_json, last_error_kind,
    last_error_message, created_at_ms, updated_at_ms
";

pub(super) fn queue_task(
    connection: &mut Connection,
    task: CodeIndexTaskSeed,
) -> Result<CodeIndexTaskRecord, StorageError> {
    super::super::retry::retry_sqlite_transient(|| queue_task_once(connection, &task))
}

fn queue_task_once(
    connection: &mut Connection,
    task: &CodeIndexTaskSeed,
) -> Result<CodeIndexTaskRecord, StorageError> {
    if let Some(existing) =
        task_by_fingerprint(connection, &task.repository_id, &task.input_fingerprint)?
        && existing.state.is_unfinished()
    {
        return Ok(existing);
    }

    let task_id = super::super::helpers::stable_id(
        "code-index-task",
        &format!("{}:{}", task.repository_id, task.input_fingerprint),
    );
    connection.execute(
        "
        INSERT INTO code_repository_index_tasks (
            task_id, repository_id, alias, ref_selector, resolved_commit_sha, tree_hash,
            source_scope, path_filters_json, language_filters_json, mode_json, state,
            attempt_count, next_retry_at_ms, input_fingerprint, resource_budget_json,
            payload_json, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'queued',
                0, ?11, ?12, ?13, ?14, ?11, ?11)
        ON CONFLICT(repository_id, input_fingerprint) DO UPDATE SET
            alias = excluded.alias,
            ref_selector = excluded.ref_selector,
            resolved_commit_sha = excluded.resolved_commit_sha,
            tree_hash = excluded.tree_hash,
            source_scope = excluded.source_scope,
            path_filters_json = excluded.path_filters_json,
            language_filters_json = excluded.language_filters_json,
            mode_json = excluded.mode_json,
            state = 'queued',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            attempt_count = 0,
            next_retry_at_ms = excluded.next_retry_at_ms,
            resource_budget_json = excluded.resource_budget_json,
            payload_json = excluded.payload_json,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![
            &task_id,
            &task.repository_id,
            &task.alias,
            &task.ref_selector,
            &task.resolved_commit_sha,
            &task.tree_hash,
            &task.source_scope,
            json(&task.path_filters)?,
            json(&task.language_filters)?,
            json(&task.mode)?,
            task.now_ms,
            &task.input_fingerprint,
            json(&task.resource_budget)?,
            &task.payload_json,
        ],
    )?;

    task_by_fingerprint(connection, &task.repository_id, &task.input_fingerprint)?
        .ok_or_else(|| StorageError::InvalidInput("code index task was not persisted".to_owned()))
}

pub(super) fn claim_task(
    connection: &mut Connection,
    request: CodeIndexTaskClaimRequest,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    super::super::retry::retry_sqlite_transient(|| claim_task_once(connection, &request))
}

fn claim_task_once(
    connection: &mut Connection,
    request: &CodeIndexTaskClaimRequest,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let lease_owner = validate_claim_request(
        &request.lease_owner,
        request.lease_duration_ms,
        request.max_attempts,
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    recover_expired_leases(&transaction, request.now_ms, request.max_attempts)?;
    let task_id = if let Some(task_id) = request.task_id.as_deref() {
        transaction
            .query_row(
                "
                SELECT candidate.task_id
                FROM code_repository_index_tasks candidate
                WHERE candidate.task_id = ?1
                  AND candidate.next_retry_at_ms <= ?2
                  AND candidate.attempt_count < ?3
                  AND candidate.state IN ('queued', 'retrying')
                ",
                params![task_id, request.now_ms, request.max_attempts],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    } else {
        transaction
            .query_row(
                "
                SELECT candidate.task_id
                FROM code_repository_index_tasks candidate
                WHERE candidate.next_retry_at_ms <= ?1
                  AND candidate.attempt_count < ?2
                  AND candidate.state IN ('queued', 'retrying')
                ORDER BY candidate.created_at_ms ASC, candidate.task_id ASC
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
        UPDATE code_repository_index_tasks
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
            &task_id,
            lease_owner,
            request.now_ms.saturating_add(request.lease_duration_ms),
            request.now_ms,
            request.max_attempts,
        ],
    )?;
    if changed == 0 {
        transaction.commit()?;
        return Ok(None);
    }
    let sql = task_select_sql("WHERE task_id = ?1");
    let task = transaction.query_row(&sql, params![&task_id], task_from_row)?;
    transaction.commit()?;

    Ok(Some(task))
}

pub(super) fn renew_task_lease(
    connection: &mut Connection,
    request: CodeIndexTaskLeaseRenewal,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let lease_owner = validate_lease_owner(&request.lease_owner)?;
    if request.lease_duration_ms == 0 {
        return Err(StorageError::InvalidInput(
            "code index task lease duration must be greater than zero".to_owned(),
        ));
    }
    let sql = task_update_returning_sql(
        "
        UPDATE code_repository_index_tasks
        SET lease_expires_at_ms = ?4,
            updated_at_ms = ?5
        WHERE task_id = ?1
          AND state = 'running'
          AND lease_owner = ?2
          AND attempt_count = ?3
          AND lease_expires_at_ms > ?5
        ",
    );
    let renewed = super::super::retry::retry_sqlite_transient(|| {
        connection
            .query_row(
                &sql,
                params![
                    &request.task_id,
                    lease_owner,
                    request.attempt_count,
                    request.now_ms.saturating_add(request.lease_duration_ms),
                    request.now_ms,
                ],
                task_from_row,
            )
            .optional()
            .map_err(StorageError::from)
    })?;

    renewed.ok_or_else(|| inactive_lease_error(&request.task_id))
}

pub(super) fn complete_task(
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
    let completed = super::super::retry::retry_sqlite_transient(|| {
        connection
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
            .map_err(StorageError::from)
    })?;

    completed.ok_or_else(|| inactive_lease_error(&request.task_id))
}

pub(super) fn fail_task(
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
    let failed = super::super::retry::retry_sqlite_transient(|| {
        connection
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
            .map_err(StorageError::from)
    })?;

    failed.ok_or_else(|| inactive_lease_error(&request.task_id))
}

pub(super) fn task_by_id(
    connection: &mut Connection,
    task_id: &str,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let sql = task_select_sql("WHERE task_id = ?1");
    connection
        .query_row(&sql, params![task_id], task_from_row)
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn active_task(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let sql = task_select_sql(
        "WHERE repository_id = ?1 AND state IN ('queued', 'running', 'retrying')
         ORDER BY created_at_ms ASC, task_id ASC LIMIT 1",
    );
    connection
        .query_row(&sql, params![repository_id], task_from_row)
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn checkpoint(
    connection: &mut Connection,
    source_scope: &str,
) -> Result<Option<CodeIndexCheckpoint>, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id, source_scope, state, total_path_count, parsed_file_count,
                   committed_file_count, committed_symbol_count, committed_reference_count,
                   committed_chunk_count, batch_count, last_path, resource_budget_json,
                   updated_at_ms
            FROM code_repository_index_checkpoints
            WHERE source_scope = ?1
            ",
            params![source_scope],
            checkpoint_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn recover_expired_task_leases(
    connection: &mut Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    super::super::retry::retry_sqlite_transient(|| {
        recover_expired_task_leases_once(connection, now_ms, max_attempts)
    })
}

fn recover_expired_task_leases_once(
    connection: &mut Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    if max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "code index task max attempts must be greater than zero".to_owned(),
        ));
    }
    recover_expired_leases(connection, now_ms, max_attempts)
}

pub(super) fn retention_status(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    let active_scope = connection
        .query_row(
            "SELECT last_indexed_scope_id FROM code_repositories WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_default();
    retention_summary(connection, repository_id, &active_scope, 2, false)
}

pub(super) fn prune_scopes(
    connection: &mut Connection,
    request: CodeScopeRetentionRequest,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    retention_summary(
        connection,
        &request.repository_id,
        &request.active_scope,
        request.retain_recent_successful_scopes,
        true,
    )
}

fn task_by_fingerprint(
    connection: &mut Connection,
    repository_id: &str,
    input_fingerprint: &str,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let sql = task_select_sql("WHERE repository_id = ?1 AND input_fingerprint = ?2");
    connection
        .query_row(
            &sql,
            params![repository_id, input_fingerprint],
            task_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn validate_claim_request(
    lease_owner: &str,
    lease_duration_ms: u64,
    max_attempts: u32,
) -> Result<&str, StorageError> {
    let lease_owner = validate_lease_owner(lease_owner)?;
    if lease_duration_ms == 0 {
        return Err(StorageError::InvalidInput(
            "code index task lease duration must be greater than zero".to_owned(),
        ));
    }
    if max_attempts == 0 {
        return Err(StorageError::InvalidInput(
            "code index task max attempts must be greater than zero".to_owned(),
        ));
    }

    Ok(lease_owner)
}

fn validate_lease_owner(lease_owner: &str) -> Result<&str, StorageError> {
    let lease_owner = lease_owner.trim();
    if lease_owner.is_empty() {
        return Err(StorageError::InvalidInput(
            "code index task lease owner must not be empty".to_owned(),
        ));
    }

    Ok(lease_owner)
}

fn recover_expired_leases(
    connection: &Connection,
    now_ms: u64,
    max_attempts: u32,
) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE code_repository_index_tasks
        SET state = CASE
                WHEN attempt_count >= ?2 THEN 'dead_letter'
                ELSE 'retrying'
            END,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?1,
            last_error_kind = 'lease_expired',
            last_error_message = 'code index task lease expired',
            updated_at_ms = ?1
        WHERE state = 'running'
          AND lease_expires_at_ms IS NOT NULL
          AND lease_expires_at_ms <= ?1
        ",
        params![now_ms, max_attempts],
    )?;

    Ok(())
}

fn retention_summary(
    connection: &mut Connection,
    repository_id: &str,
    active_scope: &str,
    retain_recent_successful_scopes: usize,
    prune: bool,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    let all_scopes = repository_scopes(connection, repository_id)?;
    let mut retained = BTreeSet::new();
    if !active_scope.is_empty() {
        retained.insert(active_scope.to_owned());
    }
    for scope in
        recent_successful_scopes(connection, repository_id, retain_recent_successful_scopes)?
    {
        retained.insert(scope);
    }
    for scope in unfinished_task_scopes(connection, repository_id)? {
        retained.insert(scope);
    }
    for scope in repository_set_member_scopes(connection, repository_id)? {
        retained.insert(scope);
    }
    let prunable = all_scopes
        .iter()
        .filter(|scope| !retained.contains(*scope))
        .cloned()
        .collect::<Vec<_>>();
    let mut pruned = Vec::new();
    if prune && !prunable.is_empty() {
        let transaction = connection.transaction()?;
        for scope in &prunable {
            delete_scope_index(&transaction, scope)?;
            transaction.execute(
                "DELETE FROM code_repository_scopes WHERE source_scope = ?1",
                params![scope],
            )?;
            transaction.execute(
                "DELETE FROM code_repository_index_checkpoints WHERE source_scope = ?1",
                params![scope],
            )?;
            pruned.push(scope.clone());
        }
        transaction.commit()?;
    }

    Ok(CodeScopeRetentionSummary {
        repository_id: repository_id.to_owned(),
        retained_scope_count: retained.len(),
        prunable_scope_count: prunable.len(),
        pruned_scope_count: pruned.len(),
        retained_scopes: retained.into_iter().collect(),
        prunable_scopes: prunable,
        pruned_scopes: pruned,
    })
}

fn repository_scopes(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT source_scope
        FROM code_repository_scopes
        WHERE repository_id = ?1
        ORDER BY source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn recent_successful_scopes(
    connection: &Connection,
    repository_id: &str,
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT scope.source_scope
        FROM code_repository_scopes scope
        LEFT JOIN code_repository_index_checkpoints checkpoint
          ON checkpoint.source_scope = scope.source_scope
        WHERE scope.repository_id = ?1
        ORDER BY coalesce(checkpoint.updated_at_ms, 0) DESC, scope.source_scope DESC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(params![repository_id, limit], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn unfinished_task_scopes(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT source_scope
        FROM code_repository_index_tasks
        WHERE repository_id = ?1 AND state IN ('queued', 'running', 'retrying')
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn repository_set_member_scopes(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT DISTINCT source_scope
        FROM code_repository_set_members
        WHERE repository_id = ?1
        ORDER BY source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| row.get::<_, String>(0))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn task_select_sql(where_clause: &str) -> String {
    format!(
        "
        SELECT {TASK_RECORD_COLUMNS}
        FROM code_repository_index_tasks
        {where_clause}
        "
    )
}

fn task_update_returning_sql(update_sql: &str) -> String {
    format!("{update_sql} RETURNING {TASK_RECORD_COLUMNS}")
}

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<CodeIndexTaskRecord> {
    let state = parse_task_state(row.get::<_, String>(10)?.as_str(), 10)?;
    let mode = serde_json::from_str(row.get::<_, String>(9)?.as_str()).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let resource_budget =
        serde_json::from_str(row.get::<_, String>(16)?.as_str()).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                16,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(CodeIndexTaskRecord {
        task_id: row.get(0)?,
        repository_id: row.get(1)?,
        alias: row.get(2)?,
        ref_selector: row.get(3)?,
        resolved_commit_sha: row.get(4)?,
        tree_hash: row.get(5)?,
        source_scope: row.get(6)?,
        path_filters: parse_json_list(row.get(7)?)?,
        language_filters: parse_json_list(row.get(8)?)?,
        mode,
        state,
        lease_owner: row.get(11)?,
        lease_expires_at_ms: row.get(12)?,
        attempt_count: row.get(13)?,
        next_retry_at_ms: row.get(14)?,
        input_fingerprint: row.get(15)?,
        resource_budget,
        payload_json: row.get(17)?,
        last_error_kind: row.get(18)?,
        last_error_message: row.get(19)?,
        created_at_ms: row.get(20)?,
        updated_at_ms: row.get(21)?,
    })
}

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<CodeIndexCheckpoint> {
    let resource_budget =
        serde_json::from_str::<CodeIndexResourceBudget>(row.get::<_, String>(11)?.as_str())
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
    Ok(CodeIndexCheckpoint {
        repository_id: row.get(0)?,
        source_scope: row.get(1)?,
        state: row.get(2)?,
        total_path_count: row.get(3)?,
        parsed_file_count: row.get(4)?,
        committed_file_count: row.get(5)?,
        committed_symbol_count: row.get(6)?,
        committed_reference_count: row.get(7)?,
        committed_chunk_count: row.get(8)?,
        batch_count: row.get(9)?,
        last_path: row.get(10)?,
        resource_budget,
        updated_at_ms: row.get(12)?,
    })
}

fn parse_task_state(value: &str, column: usize) -> rusqlite::Result<CodeIndexTaskState> {
    CodeIndexTaskState::parse(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn inactive_lease_error(task_id: &str) -> StorageError {
    StorageError::InvalidInput(format!(
        "code index task '{task_id}' is not held by an active lease"
    ))
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}
