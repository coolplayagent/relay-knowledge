//! Shared durable-publication fixtures for task lifecycle tests.

use rusqlite::{Connection, params};

use crate::{
    domain::CodeIndexTaskRecord,
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskFailure, SqliteGraphStore,
        StorageError, StorageFuture,
    },
};

impl SqliteGraphStore {
    /// Deterministically expires one running task lease for cross-layer recovery tests.
    pub(crate) fn expire_code_index_task_lease_for_test(
        &self,
        task_id: String,
    ) -> StorageFuture<'_, ()> {
        self.run(move |connection| {
            let changed = connection.execute(
                "UPDATE code_repository_index_tasks
                 SET lease_expires_at_ms = 0
                 WHERE task_id = ?1 AND state = 'running'",
                [&task_id],
            )?;
            if changed != 1 {
                return Err(StorageError::Invariant(format!(
                    "task lease fixture expected one running task for '{task_id}', updated {changed}"
                )));
            }
            Ok(())
        })
    }
}

pub(super) fn claim_task_at_request_time(
    connection: &mut Connection,
    request: CodeIndexTaskClaimRequest,
) -> Result<Option<CodeIndexTaskRecord>, StorageError> {
    let execution_now_ms = request.now_ms;
    super::claim_task_at(connection, request, execution_now_ms)
}

pub(super) fn complete_task_at_request_time(
    connection: &mut Connection,
    request: CodeIndexTaskCompletion,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let execution_now_ms = request.now_ms;
    super::complete_task_at(connection, request, execution_now_ms)
}

pub(super) fn fail_task_at_request_time(
    connection: &mut Connection,
    request: CodeIndexTaskFailure,
) -> Result<CodeIndexTaskRecord, StorageError> {
    let execution_now_ms = request.now_ms;
    super::fail_task_at(connection, request, execution_now_ms)
}

pub(super) fn persist_published_task_target(
    connection: &mut Connection,
    task: &CodeIndexTaskRecord,
) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT OR REPLACE INTO code_repository_scopes (
             source_scope, repository_id, resolved_commit_sha, tree_hash,
             path_filters_json, language_filters_json, indexed_file_count,
             symbol_count, reference_count, chunk_count, stale, degraded_reason
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0, 0, 0, NULL)",
        params![
            task.source_scope,
            task.repository_id,
            task.resolved_commit_sha,
            task.tree_hash,
            serde_json::to_string(&task.path_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            serde_json::to_string(&task.language_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
        ],
    )?;
    transaction.execute(
        "UPDATE code_repositories
         SET last_indexed_scope_id = ?2, last_indexed_commit = ?3, tree_hash = ?4,
             state = 'fresh', stale = 0
         WHERE repository_id = ?1",
        params![
            task.repository_id,
            task.source_scope,
            task.resolved_commit_sha,
            task.tree_hash,
        ],
    )?;
    transaction.execute(
        "INSERT OR REPLACE INTO software_global_status (
             source_scope, repository_id, projected_graph_version, stale,
             component_count, sdk_usage_count
         ) VALUES (?1, ?2, 1, 0, 0, 0)",
        params![task.source_scope, task.repository_id,],
    )?;
    crate::storage::sqlite::code::record_receipt_from_active_fence(
        &transaction,
        &task.source_scope,
    )?;
    transaction.commit()?;
    Ok(())
}
