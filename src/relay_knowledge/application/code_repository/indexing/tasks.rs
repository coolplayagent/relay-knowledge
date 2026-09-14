use std::sync::atomic::Ordering;

use crate::{
    api::{
        ApiError, ApiMetadata, CodeIndexWorkerStatus, CodeRepositoryIndexResetResponse,
        RequestContext,
    },
    application::service::RelayKnowledgeService,
};

use super::super::{
    clock::now_millis,
    errors::storage_api_error,
    repository::{code_status_checkpoint, required_code_repository},
};
use super::state::RETAIN_RECENT_CODE_SCOPES;
use super::task::recover_orphaned_code_index_task_leases;

impl RelayKnowledgeService {
    /// Runs one bounded, restart-safe retention pass for persistent leftovers.
    pub(crate) async fn run_code_scope_retention_once(&self) -> Result<bool, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        store
            .schedule_code_repository_retention(
                self.runtime.workers.code_index_max_indexed_repositories,
                now_millis(),
            )
            .await
            .map_err(storage_api_error)?;
        let repository_scan_pending = store
            .code_repository_retention_scan_pending()
            .await
            .map_err(storage_api_error)?;
        let repositories = store
            .list_code_repositories()
            .await
            .map_err(storage_api_error)?;
        if repositories.is_empty() {
            return Ok(false);
        }
        let repository_count = repositories.len();
        let start = self.code_retention_cursor.fetch_add(1, Ordering::Relaxed) % repository_count;
        let mut first_error = None;
        let mut pending_repository = None;
        for status in repositories
            .into_iter()
            .cycle()
            .skip(start)
            .take(repository_count)
        {
            let retention = match store
                .code_scope_retention(status.repository_id.clone())
                .await
            {
                Ok(retention) => retention,
                Err(error) => {
                    first_error.get_or_insert_with(|| storage_api_error(error));
                    continue;
                }
            };
            if retention.maintenance_pending && pending_repository.is_none() {
                pending_repository = Some((
                    status.repository_id,
                    status.last_indexed_scope_id.unwrap_or_default(),
                ));
            }
        }
        let maintenance_active = if let Some((repository_id, active_scope)) = pending_repository {
            match store
                .prune_code_repository_scopes(crate::storage::CodeScopeRetentionRequest {
                    repository_id,
                    active_scope,
                    retain_recent_successful_scopes: RETAIN_RECENT_CODE_SCOPES,
                    repository_retention_cutoff_ms: None,
                    repository_retention_cutoff_generation: None,
                    repository_retention_initial_scope: None,
                })
                .await
            {
                Ok(pruned) => {
                    pruned.pruned_scope_count > 0
                        || pruned.retiring_job_count > 0
                        || pruned.maintenance_pending
                }
                Err(error) => {
                    first_error.get_or_insert_with(|| storage_api_error(error));
                    false
                }
            }
        } else {
            false
        };
        first_error.map_or(Ok(repository_scan_pending || maintenance_active), Err)
    }

    /// Resets unfinished full index tasks for a registered repository.
    pub async fn reset_code_repository_index_tasks(
        &self,
        repository: String,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexResetResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(store.as_ref(), &repository).await?;
        // Reclaim leases whose owner process has exited before applying the
        // operator reset. This keeps reset idempotent while preserving the
        // single-writer invariant for genuinely live workers.
        recover_orphaned_code_index_task_leases(
            &store,
            now_millis(),
            &self.runtime.process.windows_tasklist_command,
        )
        .await?;
        let reset_tasks = store
            .reset_code_index_tasks(status.repository_id.clone(), now_millis())
            .await
            .map_err(storage_api_error)?;
        let active_task = store
            .active_code_index_task(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?;
        let checkpoint =
            code_status_checkpoint(store.as_ref(), &status, active_task.as_ref()).await?;
        let retention = store
            .code_scope_retention(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositoryIndexResetResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
            reset_task_count: reset_tasks.len(),
            reset_tasks,
            active_task,
            checkpoint,
            retention,
        })
    }

    /// Reconciles expired or orphaned repository index leases before resident workers start.
    pub async fn reconcile_startup_code_index_tasks(&self) -> Result<(), ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        recover_orphaned_code_index_task_leases(
            &store,
            now_millis(),
            &self.runtime.process.windows_tasklist_command,
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn code_index_worker_status(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    ) -> Result<CodeIndexWorkerStatus, ApiError> {
        recover_orphaned_code_index_task_leases(
            store,
            now_millis(),
            &self.runtime.process.windows_tasklist_command,
        )
        .await?;
        self.read_only_code_index_worker_status(store).await
    }

    pub(crate) async fn read_only_code_index_worker_status(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    ) -> Result<CodeIndexWorkerStatus, ApiError> {
        let queue = store
            .code_index_task_queue_status()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeIndexWorkerStatus::from_queue(
            self.runtime.workers.code_index_max_in_flight,
            queue,
        ))
    }
}
