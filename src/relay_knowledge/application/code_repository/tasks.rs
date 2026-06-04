use crate::{
    api::{
        ApiError, ApiMetadata, CodeIndexWorkerStatus, CodeRepositoryIndexResetResponse,
        RequestContext,
    },
    application::service::RelayKnowledgeService,
};

use super::support::{
    code_status_checkpoint, now_millis, recover_orphaned_code_index_task_leases,
    required_code_repository, storage_api_error,
};

impl RelayKnowledgeService {
    /// Resets unfinished full index tasks for a registered repository.
    pub async fn reset_code_repository_index_tasks(
        &self,
        repository: String,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexResetResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &repository).await?;
        let reset_tasks = store
            .reset_code_index_tasks(status.repository_id.clone(), now_millis())
            .await
            .map_err(storage_api_error)?;
        let active_task = store
            .active_code_index_task(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?;
        let checkpoint = code_status_checkpoint(&store, &status, active_task.as_ref()).await?;
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
        recover_orphaned_code_index_task_leases(&store, now_millis())
            .await
            .map(|_| ())
    }

    pub(crate) async fn code_index_worker_status(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    ) -> Result<CodeIndexWorkerStatus, ApiError> {
        recover_orphaned_code_index_task_leases(store, now_millis()).await?;
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
