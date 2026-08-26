//! Owns lease-guarded code-index task completion and failure transitions.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    domain::{CodeIndexTaskRecord, CodeIndexTaskState},
    storage::{CodeIndexTaskCompletion, CodeIndexTaskFailure, StorageError},
};

use super::{
    lease::{
        inactive_lease_error, system_now_millis, validate_lease_owner,
        validate_observed_execution_time,
    },
    record_mapping::{task_from_row, task_update_returning_sql},
};

pub(in crate::storage::sqlite::code) fn publication_receipt(
    connection: &mut Connection,
    task_id: &str,
    repository_id: &str,
    source_scope: &str,
    now_ms: u64,
) -> Result<bool, StorageError> {
    publication_target_has_receipt(
        connection,
        task_id,
        repository_id,
        source_scope,
        Some(now_ms),
    )
}

pub(in crate::storage::sqlite::code) fn complete_task(
    connection: &mut Connection,
    request: CodeIndexTaskCompletion,
) -> Result<CodeIndexTaskRecord, StorageError> {
    complete_task_with_clock(connection, request, system_now_millis)
}

fn complete_task_with_clock(
    connection: &mut Connection,
    request: CodeIndexTaskCompletion,
    mut clock: impl FnMut() -> Result<u64, StorageError>,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let lease_owner = validate_lease_owner(&request.lease_owner)?.to_owned();
    let sql = task_update_returning_sql(
        "
        UPDATE code_repository_index_tasks AS task
        SET state = 'succeeded',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            last_error_kind = NULL,
            last_error_message = NULL,
            updated_at_ms = ?5
        WHERE task.task_id = ?1
          AND task.state = 'running'
          AND task.lease_owner = ?2
          AND task.attempt_count = ?3
          AND task.publication_generation = ?4
          AND task.lease_expires_at_ms > ?5
          AND EXISTS (
              SELECT 1
              FROM code_repository_publication_fences authority
              WHERE authority.repository_id = task.repository_id
                AND authority.task_id = task.task_id
                AND authority.lease_owner = ?2
                AND authority.attempt_count = ?3
                AND authority.generation = ?4
          )
        ",
    );
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let execution_now_ms = clock()?;
        validate_observed_execution_time(request.now_ms, execution_now_ms)?;
        let completed = transaction
            .query_row(
                &sql,
                params![
                    &request.task_id,
                    &lease_owner,
                    request.attempt_count,
                    request.publication_generation,
                    execution_now_ms,
                ],
                task_from_row,
            )
            .optional()
            .map_err(StorageError::from)?
            .ok_or_else(|| inactive_lease_error(&request.task_id))?;
        validate_publication_receipt(&transaction, &completed)?;
        super::retention::prune_finished_task_history(
            &transaction,
            &completed.repository_id,
            Some(&completed.task_id),
        )?;
        transaction.commit()?;
        Ok(completed)
    })
}

fn validate_publication_receipt(
    transaction: &rusqlite::Transaction<'_>,
    completed: &CodeIndexTaskRecord,
) -> Result<(), StorageError> {
    let published = publication_target_has_receipt(
        transaction,
        &completed.task_id,
        &completed.repository_id,
        &completed.source_scope,
        None,
    )?;
    if !published {
        return Err(StorageError::InvalidInput(format!(
            "code index task '{}' cannot complete before its target scope is durably published",
            completed.task_id
        )));
    }
    Ok(())
}

/// Proves that a receipt still describes the task's current authoritative
/// target. Receipt generations intentionally do not participate: an attempt
/// that published and then lost its lease may be reclaimed without rewriting
/// the same durable target.
fn publication_target_has_receipt(
    connection: &Connection,
    task_id: &str,
    repository_id: &str,
    source_scope: &str,
    active_lease_after_ms: Option<u64>,
) -> Result<bool, StorageError> {
    let target_matches = connection
        .query_row(
            "
            SELECT 1
            FROM code_repository_publication_receipts receipt
            JOIN code_repository_index_tasks task
              ON task.task_id = receipt.task_id
             AND task.repository_id = receipt.repository_id
             AND task.source_scope = receipt.source_scope
            JOIN code_repositories repository
              ON repository.repository_id = task.repository_id
             AND repository.last_indexed_scope_id = task.source_scope
             AND repository.last_indexed_commit = task.resolved_commit_sha
             AND repository.tree_hash = task.tree_hash
             AND repository.state = 'fresh' AND repository.stale = 0
            JOIN code_repository_scopes scope
              ON scope.source_scope = task.source_scope
             AND scope.repository_id = task.repository_id
             AND scope.tree_hash = task.tree_hash
             AND scope.path_filters_json = task.path_filters_json
             AND scope.language_filters_json = task.language_filters_json
             AND scope.stale = 0 AND scope.retiring = 0
            WHERE task.task_id = ?1
              AND task.repository_id = ?2
              AND task.source_scope = ?3
              AND (
                  (?4 IS NULL AND task.state = 'succeeded')
                  OR (
                      ?4 IS NOT NULL
                      AND task.state = 'running'
                      AND task.lease_owner IS NOT NULL
                      AND task.lease_expires_at_ms > ?4
                  )
              )
              AND (
                  scope.resolved_commit_sha = task.resolved_commit_sha
                  OR EXISTS (
                      SELECT 1
                      FROM code_repository_commit_scopes commit_scope
                      WHERE commit_scope.repository_id = task.repository_id
                        AND commit_scope.resolved_commit_sha = task.resolved_commit_sha
                        AND commit_scope.source_scope = task.source_scope
                  )
              )
              AND (
                  NOT EXISTS (
                      SELECT 1 FROM code_repository_index_checkpoints checkpoint
                      WHERE checkpoint.source_scope = task.source_scope
                  )
                  OR EXISTS (
                      SELECT 1 FROM code_repository_index_checkpoints checkpoint
                      WHERE checkpoint.source_scope = task.source_scope
                        AND checkpoint.repository_id = task.repository_id
                        AND checkpoint.state = 'completed'
                        AND checkpoint.tree_hash = task.tree_hash
                        AND checkpoint.path_filters_json = task.path_filters_json
                        AND checkpoint.language_filters_json = task.language_filters_json
                        AND (
                            checkpoint.resolved_commit_sha = task.resolved_commit_sha
                            OR checkpoint.resolved_commit_sha = scope.resolved_commit_sha
                            OR EXISTS (
                                SELECT 1
                                FROM code_repository_commit_scopes checkpoint_commit
                                WHERE checkpoint_commit.repository_id = task.repository_id
                                  AND checkpoint_commit.resolved_commit_sha =
                                      checkpoint.resolved_commit_sha
                                  AND checkpoint_commit.source_scope = task.source_scope
                            )
                        )
                  )
              )
            ",
            params![task_id, repository_id, source_scope, active_lease_after_ms],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !target_matches {
        return Ok(false);
    }
    publication_software_is_fresh(connection, repository_id, source_scope)
}

fn publication_software_is_fresh(
    connection: &Connection,
    repository_id: &str,
    source_scope: &str,
) -> Result<bool, StorageError> {
    let local_status = connection
        .query_row(
            "SELECT repository_id = ?1 AND stale = 0
             FROM software_global_status WHERE source_scope = ?2",
            params![repository_id, source_scope],
            |row| row.get::<_, bool>(0),
        )
        .optional()?;
    if let Some(fresh) = local_status {
        if !fresh {
            return Ok(false);
        }
        return connection
            .query_row(
                "SELECT business.repository_id = ?1 AND business.stale = 0 AND (
                    business.resolved_commit_sha = (
                        SELECT resolved_commit_sha
                        FROM code_repository_scopes
                        WHERE source_scope = ?2
                    )
                    OR EXISTS (
                        SELECT 1
                        FROM code_repository_commit_scopes alias
                        WHERE alias.repository_id = ?1
                          AND alias.source_scope = ?2
                          AND alias.resolved_commit_sha = business.resolved_commit_sha
                    )
                 )
                 FROM business_knowledge_status business
                 WHERE business.source_scope = ?2",
                params![repository_id, source_scope],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map(|fresh| fresh.unwrap_or(false))
            .map_err(StorageError::from);
    }

    let has_partition_catalog = connection
        .query_row(
            "SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'storage_repository_shard_scopes'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !has_partition_catalog {
        return Ok(false);
    }
    connection
        .query_row(
            "SELECT 1
             FROM storage_repository_shard_scopes scope
             JOIN storage_repository_shards shard
               ON shard.repository_id = scope.repository_id
             WHERE scope.source_scope = ?2
               AND scope.repository_id = ?1
               AND scope.state = 'active'
               AND shard.state = 'active'",
            params![repository_id, source_scope],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code) fn fail_task(
    connection: &mut Connection,
    request: CodeIndexTaskFailure,
) -> Result<CodeIndexTaskRecord, StorageError> {
    fail_task_with_clock(connection, request, system_now_millis)
}

fn fail_task_with_clock(
    connection: &mut Connection,
    request: CodeIndexTaskFailure,
    mut clock: impl FnMut() -> Result<u64, StorageError>,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let lease_owner = validate_lease_owner(&request.lease_owner)?.to_owned();
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
        UPDATE code_repository_index_tasks AS task
        SET state = ?5,
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            next_retry_at_ms = ?6,
            last_error_kind = ?7,
            last_error_message = ?8,
            updated_at_ms = ?9
        WHERE task.task_id = ?1
          AND task.state = 'running'
          AND task.lease_owner = ?2
          AND task.attempt_count = ?3
          AND task.publication_generation = ?4
          AND task.lease_expires_at_ms > ?9
          AND EXISTS (
              SELECT 1
              FROM code_repository_publication_fences authority
              WHERE authority.repository_id = task.repository_id
                AND authority.task_id = task.task_id
                AND authority.lease_owner = ?2
                AND authority.attempt_count = ?3
                AND authority.generation = ?4
          )
        ",
    );
    let failed = super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let execution_now_ms = clock()?;
        validate_observed_execution_time(request.now_ms, execution_now_ms)?;
        let failed = transaction
            .query_row(
                &sql,
                params![
                    &request.task_id,
                    &lease_owner,
                    request.attempt_count,
                    request.publication_generation,
                    next_state.as_str(),
                    execution_now_ms.saturating_add(request.retry_backoff_ms),
                    &request.error_kind,
                    &request.error_message,
                    execution_now_ms,
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
pub(super) fn complete_task_at(
    connection: &mut Connection,
    request: CodeIndexTaskCompletion,
    execution_now_ms: u64,
) -> Result<CodeIndexTaskRecord, StorageError> {
    complete_task_with_clock(connection, request, || Ok(execution_now_ms))
}

#[cfg(test)]
pub(super) fn fail_task_at(
    connection: &mut Connection,
    request: CodeIndexTaskFailure,
    execution_now_ms: u64,
) -> Result<CodeIndexTaskRecord, StorageError> {
    fail_task_with_clock(connection, request, || Ok(execution_now_ms))
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "completion_clock_tests.rs"]
mod clock_tests;
