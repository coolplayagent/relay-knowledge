//! One-shot durable index-worker claim, completion, retry, and recovery.

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse, RequestContext},
    application::service::RelayKnowledgeService,
    domain::{CodeIndexMode, CodeIndexRequest, CodeIndexTaskRecord},
    storage::{
        CodeIndexTaskClaimRequest, CodeIndexTaskCompletion, CodeIndexTaskFailure,
        CodeScopeRetentionRequest,
    },
};

use super::{
    super::{
        clock::now_millis, errors::storage_api_error,
        worktree_ref::pending_worktree_overlay_base_commit,
    },
    state::RETAIN_RECENT_CODE_SCOPES,
    task::{
        CODE_INDEX_TASK_LEASE_MS, CODE_INDEX_TASK_MAX_ATTEMPTS, CODE_INDEX_TASK_RETRY_BACKOFF_MS,
        CodeIndexTaskLeaseContext, code_index_task_failure_disposition,
        code_index_worker_lease_owner, recover_orphaned_code_index_task_leases,
        refresh_code_index_task_lease,
    },
};

impl RelayKnowledgeService {
    /// Runs one queued code index task under a lease.
    pub async fn run_code_index_task_once(
        &self,
        task_id: Option<String>,
        context: RequestContext,
    ) -> Result<Option<CodeIndexTaskRecord>, ApiError> {
        self.run_code_index_task_once_with_response(task_id, context)
            .await
            .map(|outcome| outcome.map(|(task, _)| task))
    }

    pub(crate) async fn run_code_index_task_once_with_response(
        &self,
        task_id: Option<String>,
        context: RequestContext,
    ) -> Result<Option<(CodeIndexTaskRecord, CodeRepositoryIndexResponse)>, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let lease_owner = code_index_worker_lease_owner();
        let Some(task) = store
            .claim_code_index_task(CodeIndexTaskClaimRequest {
                task_id,
                lease_owner: lease_owner.clone(),
                lease_duration_ms: CODE_INDEX_TASK_LEASE_MS,
                max_attempts: CODE_INDEX_TASK_MAX_ATTEMPTS,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?
        else {
            return Ok(None);
        };
        let mut request = match serde_json::from_str::<CodeIndexRequest>(&task.payload_json) {
            Ok(request) => request,
            Err(error) => {
                let message = format!(
                    "code index task '{}' payload is invalid: {error}",
                    task.task_id
                );
                let _ = store
                    .fail_code_index_task(CodeIndexTaskFailure {
                        task_id: task.task_id,
                        lease_owner,
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        error_kind: "task_payload".to_owned(),
                        error_message: message.clone(),
                        retry_backoff_ms: CODE_INDEX_TASK_RETRY_BACKOFF_MS,
                        max_attempts: CODE_INDEX_TASK_MAX_ATTEMPTS,
                        now_ms: now_millis(),
                    })
                    .await;
                return Err(ApiError::invalid_argument(message));
            }
        };
        if task.mode == CodeIndexMode::WorktreeOverlay {
            if let Some(base_commit) =
                pending_worktree_overlay_base_commit(&task.resolved_commit_sha)
            {
                request.repository.ref_selector = base_commit.to_owned();
            }
        } else if task.mode == CodeIndexMode::Full {
            request.repository.ref_selector = task.resolved_commit_sha.clone();
        } else {
            request.repository.ref_selector = task.ref_selector.clone();
        }
        let lease_context = CodeIndexTaskLeaseContext {
            task_id: task.task_id.clone(),
            lease_owner: lease_owner.clone(),
            attempt_count: task.attempt_count,
            lease_duration_ms: CODE_INDEX_TASK_LEASE_MS,
            publication_fence: crate::domain::CodeIndexPublicationFence {
                repository_id: task.repository_id.clone(),
                task_id: task.task_id.clone(),
                lease_owner: lease_owner.clone(),
                attempt_count: task.attempt_count,
                generation: task.publication_generation,
            },
            source_scope: task.source_scope.clone(),
            resolved_commit_sha: task.resolved_commit_sha.clone(),
            tree_hash: task.tree_hash.clone(),
            path_filters: task.path_filters.clone(),
            language_filters: task.language_filters.clone(),
            resource_budget: task.resource_budget,
        };
        let result = self
            .index_code_repository_inner(request, context, Some(lease_context.clone()))
            .await;
        match result {
            Ok(response) => {
                refresh_code_index_task_lease(&store, Some(&lease_context)).await?;
                let completed = store
                    .complete_code_index_task(CodeIndexTaskCompletion {
                        task_id: task.task_id.clone(),
                        lease_owner,
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        now_ms: now_millis(),
                    })
                    .await
                    .map_err(storage_api_error)?;
                if let Err(error) = store
                    .run_code_index_post_maintenance(
                        response.summary.repository_id.clone(),
                        response.summary.source_scope.clone(),
                    )
                    .await
                {
                    tracing::warn!(
                        task_id = %completed.task_id,
                        error = %error,
                        "code index completed but post-index SQLite maintenance did not run"
                    );
                }
                if let Err(error) = store
                    .prune_code_repository_scopes(CodeScopeRetentionRequest {
                        repository_id: response.summary.repository_id.clone(),
                        active_scope: response.summary.source_scope.clone(),
                        retain_recent_successful_scopes: RETAIN_RECENT_CODE_SCOPES,
                        repository_retention_cutoff_ms: None,
                        repository_retention_cutoff_generation: None,
                        repository_retention_initial_scope: None,
                    })
                    .await
                {
                    tracing::warn!(
                        task_id = %completed.task_id,
                        error = %error,
                        "code index published but bounded scope retention did not complete"
                    );
                }
                Ok(Some((completed, response)))
            }
            Err(error) => {
                let failure = code_index_task_failure_disposition(&error, task.attempt_count);
                let _ = store
                    .fail_code_index_task(CodeIndexTaskFailure {
                        task_id: task.task_id,
                        lease_owner,
                        attempt_count: task.attempt_count,
                        publication_generation: task.publication_generation,
                        error_kind: failure.error_kind.to_owned(),
                        error_message: error.message.clone(),
                        retry_backoff_ms: CODE_INDEX_TASK_RETRY_BACKOFF_MS,
                        max_attempts: failure.max_attempts,
                        now_ms: now_millis(),
                    })
                    .await;
                Err(error)
            }
        }
    }

    /// Recovers code-index worker leases that belonged to exited service processes.
    pub(crate) async fn recover_orphaned_code_index_tasks_on_startup(
        &self,
    ) -> Result<usize, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        recover_orphaned_code_index_task_leases(
            &store,
            now_millis(),
            &self.runtime.process.windows_tasklist_command,
        )
        .await
    }
}
