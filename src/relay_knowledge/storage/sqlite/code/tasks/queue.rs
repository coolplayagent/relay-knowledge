//! Owns idempotent durable code-index task queue persistence.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::CodeIndexTaskRecord,
    storage::{CodeIndexTaskSeed, StorageError},
};

use super::record_mapping::{task_from_row, task_select_sql};

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
    if let Some(existing) =
        task_by_fingerprint(connection, &task.repository_id, &task.input_fingerprint)?
        && existing.state.is_unfinished()
    {
        return Ok(existing);
    }

    let task_id = super::super::super::evidence_identity::stable_id(
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

fn json<T: serde::Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
