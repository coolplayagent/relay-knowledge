//! Owns idempotent durable code-index task queue persistence.

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{
    domain::{CodeIndexMode, CodeIndexTaskRecord, CodeIndexTaskState},
    storage::{CodeIndexTaskSeed, StorageError},
};

use super::record_mapping::{task_from_row, task_select_sql};
use super::worktree::{compatible_non_retiring_scopes_for_commit, worktree_task_base_commit};

#[cfg(test)]
use super::scope_capacity::MAX_SCOPE_SLOTS_PER_REPOSITORY;

/// Maximum durable code-index work that may pin scopes for one repository.
const MAX_UNFINISHED_TASKS_PER_REPOSITORY: usize = 32;
/// Maximum durable unfinished code-index work across the control database.
const MAX_UNFINISHED_TASKS_GLOBAL: usize = 256;

pub(in crate::storage::sqlite::code) fn queue_task(
    connection: &mut Connection,
    task: CodeIndexTaskSeed,
) -> Result<CodeIndexTaskRecord, StorageError> {
    super::super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        queue_task_once(connection, &task)
    })
}

fn queue_task_once(
    connection: &mut Connection,
    task: &CodeIndexTaskSeed,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let queued = queue_task_in_transaction(&transaction, task)?;
    super::retention::prune_finished_task_history(
        &transaction,
        &task.repository_id,
        Some(&queued.task_id),
    )?;
    transaction.commit()?;

    Ok(queued)
}

fn queue_task_in_transaction(
    transaction: &Transaction<'_>,
    task: &CodeIndexTaskSeed,
) -> Result<CodeIndexTaskRecord, StorageError> {
    super::retention_gc::reject_retiring_scope(transaction, &task.source_scope)?;
    if let Some(existing) =
        task_by_fingerprint(transaction, &task.repository_id, &task.input_fingerprint)?
    {
        if existing.state.is_unfinished() {
            require_compatible_non_retiring_bases(transaction, task)?;
            supersede_pending_worktree_tasks(transaction, task, Some(&task.input_fingerprint))?;
            return Ok(existing);
        }
        if existing.state == CodeIndexTaskState::Succeeded
            && existing.payload_json == task.payload_json
            && periodic_worktree_reconcile_payload(&task.payload_json)
        {
            return Ok(existing);
        }
        if existing.state == CodeIndexTaskState::DeadLetter
            && existing.payload_json == task.payload_json
        {
            if matches!(&task.mode, CodeIndexMode::Incremental { .. }) {
                supersede_pending_worktree_tasks(transaction, task, None)?;
            }
            return Ok(existing);
        }
    }
    require_compatible_non_retiring_bases(transaction, task)?;
    reject_worktree_behind_unfinished_commit(transaction, task)?;
    supersede_pending_worktree_tasks(transaction, task, None)?;
    enforce_unfinished_task_capacity(transaction, &task.repository_id)?;
    super::scope_capacity::enforce_new_target(
        transaction,
        &task.repository_id,
        &task.source_scope,
        &task.mode,
    )?;
    let created_at_ms =
        next_repository_queue_timestamp(transaction, &task.repository_id, task.now_ms)?;

    let task_id = super::super::super::evidence_identity::stable_id(
        "code-index-task",
        &format!("{}:{}", task.repository_id, task.input_fingerprint),
    );
    transaction.execute(
        "
        INSERT INTO code_repository_index_tasks (
            task_id, repository_id, alias, ref_selector, resolved_commit_sha, tree_hash,
            source_scope, path_filters_json, language_filters_json, mode_json, state,
            attempt_count, next_retry_at_ms, input_fingerprint, resource_budget_json,
            payload_json, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'queued',
                0, ?11, ?12, ?13, ?14, ?15, ?11)
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
            publication_generation = 0,
            next_retry_at_ms = excluded.next_retry_at_ms,
            resource_budget_json = excluded.resource_budget_json,
            payload_json = excluded.payload_json,
            last_error_kind = NULL,
            last_error_message = NULL,
            created_at_ms = excluded.created_at_ms,
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
            created_at_ms,
        ],
    )?;

    task_by_fingerprint(transaction, &task.repository_id, &task.input_fingerprint)?
        .ok_or_else(|| StorageError::InvalidInput("code index task was not persisted".to_owned()))
}

fn periodic_worktree_reconcile_payload(payload: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .pointer("/watcher/kind")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("periodic_worktree_reconcile")
}

fn reject_worktree_behind_unfinished_commit(
    transaction: &Transaction<'_>,
    task: &CodeIndexTaskSeed,
) -> Result<(), StorageError> {
    if task.mode != CodeIndexMode::WorktreeOverlay {
        return Ok(());
    }
    let base_commit = worktree_task_base_commit(&task.resolved_commit_sha, &task.ref_selector)
        .unwrap_or_default();
    let stale = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1
             FROM code_repository_index_tasks
             WHERE repository_id = ?1
               AND state IN ('queued', 'running', 'retrying')
               AND mode_json LIKE '{\"incremental\":%'
               AND resolved_commit_sha <> ?2
             LIMIT 1
         )",
        params![task.repository_id, base_commit],
        |row| row.get::<_, bool>(0),
    )?;
    if stale {
        return Err(StorageError::CapacityExceeded(format!(
            "worktree overlay for repository '{}' is pinned to commit '{base_commit}' while an immutable commit update is unfinished; retry after the managed commit task publishes",
            task.alias
        )));
    }
    Ok(())
}

fn next_repository_queue_timestamp(
    transaction: &Transaction<'_>,
    repository_id: &str,
    requested_at_ms: u64,
) -> Result<u64, StorageError> {
    let latest = transaction
        .query_row(
            "SELECT created_at_ms
         FROM code_repository_index_tasks
         WHERE repository_id = ?1
         ORDER BY created_at_ms DESC, task_id DESC
         LIMIT 1",
            [repository_id],
            |row| row.get::<_, u64>(0),
        )
        .optional()?;
    Ok(latest
        .map(|timestamp| timestamp.saturating_add(1))
        .unwrap_or_default()
        .max(requested_at_ms))
}

fn require_compatible_non_retiring_bases(
    transaction: &Transaction<'_>,
    task: &CodeIndexTaskSeed,
) -> Result<(), StorageError> {
    // Queue admission and logical GC retirement both run under BEGIN IMMEDIATE.
    // A successful insert therefore turns the base into a durable retention pin
    // before a maintenance pass can schedule that scope for deletion.
    let path_filters_json = json(&task.path_filters)?;
    let language_filters_json = json(&task.language_filters)?;
    for base_commit in task_base_commits(task)? {
        if compatible_non_retiring_scopes_for_commit(
            transaction,
            &task.repository_id,
            &base_commit,
            &path_filters_json,
            &language_filters_json,
        )?
        .is_empty()
        {
            return Err(StorageError::InvalidInput(format!(
                "code index task base commit '{base_commit}' has no compatible non-retiring scope for repository '{}'; wait for bounded maintenance to complete, then retry or run a full index",
                task.alias
            )));
        }
    }
    Ok(())
}

fn task_base_commits(task: &CodeIndexTaskSeed) -> Result<Vec<String>, StorageError> {
    match &task.mode {
        CodeIndexMode::Incremental { base_ref, .. } => Ok(vec![base_ref.clone()]),
        CodeIndexMode::WorktreeOverlay => {
            let base_commit =
                worktree_task_base_commit(&task.resolved_commit_sha, &task.ref_selector)
                .ok_or_else(|| {
                    StorageError::InvalidInput(format!(
                        "worktree overlay task for repository '{}' has no pinned clean base commit; retry or run a full index",
                        task.alias
                    ))
                })?;
            Ok(vec![base_commit.to_owned()])
        }
        CodeIndexMode::Full => Ok(Vec::new()),
    }
}

fn supersede_pending_worktree_tasks(
    transaction: &Transaction<'_>,
    task: &CodeIndexTaskSeed,
    retained_fingerprint: Option<&str>,
) -> Result<(), StorageError> {
    if !matches!(
        &task.mode,
        CodeIndexMode::WorktreeOverlay | CodeIndexMode::Incremental { .. }
    ) {
        return Ok(());
    }
    let worktree_mode_json = json(&CodeIndexMode::WorktreeOverlay)?;
    let retained_fingerprint = retained_fingerprint.unwrap_or("");
    transaction.execute(
        "
        UPDATE code_repository_index_tasks
        SET state = 'cancelled',
            lease_owner = NULL,
            lease_expires_at_ms = NULL,
            last_error_kind = 'superseded',
            last_error_message = ?5,
            updated_at_ms = ?4
        WHERE repository_id = ?1
          AND mode_json = ?2
          AND state IN ('queued', 'retrying')
          AND (?3 = '' OR input_fingerprint <> ?3)
        ",
        params![
            &task.repository_id,
            worktree_mode_json,
            retained_fingerprint,
            task.now_ms,
            match &task.mode {
                CodeIndexMode::Incremental { .. } => {
                    "superseded by an immutable Git commit reconciliation"
                }
                CodeIndexMode::WorktreeOverlay => {
                    "superseded by a newer bounded worktree observation"
                }
                CodeIndexMode::Full => unreachable!("full tasks return before supersession"),
            },
        ],
    )?;

    Ok(())
}

fn enforce_unfinished_task_capacity(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<(), StorageError> {
    let repository_depth = transaction.query_row(
        "
        SELECT COUNT(*) FROM (
            SELECT 1
            FROM code_repository_index_tasks
            WHERE repository_id = ?1
              AND state IN ('queued', 'running', 'retrying')
            LIMIT ?2
        )
        ",
        params![repository_id, MAX_UNFINISHED_TASKS_PER_REPOSITORY as i64],
        |row| row.get::<_, usize>(0),
    )?;
    if repository_depth >= MAX_UNFINISHED_TASKS_PER_REPOSITORY {
        return Err(StorageError::CapacityExceeded(format!(
            "code index task queue for repository '{repository_id}' has {repository_depth} unfinished tasks (capacity {MAX_UNFINISHED_TASKS_PER_REPOSITORY}); retry after queued work completes"
        )));
    }

    let global_depth = transaction.query_row(
        "
        SELECT COUNT(*) FROM (
            SELECT 1
            FROM code_repository_index_tasks
            WHERE state IN ('queued', 'running', 'retrying')
            LIMIT ?1
        )
        ",
        [MAX_UNFINISHED_TASKS_GLOBAL as i64],
        |row| row.get::<_, usize>(0),
    )?;
    if global_depth >= MAX_UNFINISHED_TASKS_GLOBAL {
        return Err(StorageError::CapacityExceeded(format!(
            "global code index task queue has {global_depth} unfinished tasks (capacity {MAX_UNFINISHED_TASKS_GLOBAL}); retry after queued work completes"
        )));
    }

    Ok(())
}

fn task_by_fingerprint(
    connection: &Connection,
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

fn json<T: serde::Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
