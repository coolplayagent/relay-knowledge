//! Durable task admission and historical-reuse planning for index requests.

use crate::{
    api::{ApiError, CodeRepositoryIndexStartResponse, RequestContext},
    application::service::RelayKnowledgeService,
    code::prepare_full_index_plan_with_workspace_detection,
    domain::{CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget},
    storage::CodeIndexTaskSeed,
};

use super::{
    super::{
        blocking::run_blocking_code,
        clock::now_millis,
        errors::storage_api_error,
        repository::{registration_from_status, required_code_repository},
    },
    fast_path::fresh_full_index_response,
    queue::{
        index_start_response_from_task, queue_incremental_index_task,
        queue_worktree_overlay_index_task,
    },
    state::{
        FullIndexReusePlan, active_full_index_task_for_request,
        historical_reuse_base_became_unavailable, index_start_from_completed,
        plan_full_index_reuse, requested_index_ref_for_response,
    },
    task::recover_code_index_task_leases,
};

impl RelayKnowledgeService {
    /// Starts a repository index request under the durable single-writer queue.
    pub async fn start_code_repository_index(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexStartResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status =
            required_code_repository(store.as_ref(), &request.repository.repository).await?;
        if let Some(response) =
            fresh_full_index_response(&store, &status, &request, &context).await?
        {
            return Ok(index_start_from_completed(response, None));
        }
        recover_code_index_task_leases(&store, now_millis()).await?;
        if matches!(&request.mode, CodeIndexMode::Incremental { .. }) {
            let requested_ref = requested_index_ref_for_response(&request);
            let task = queue_incremental_index_task(&store, &status, &request).await?;
            return index_start_response_from_task(&store, status, task, requested_ref, &context)
                .await;
        }
        if request.mode == CodeIndexMode::WorktreeOverlay {
            let requested_ref = requested_index_ref_for_response(&request);
            let task = queue_worktree_overlay_index_task(&store, &status, &request).await?;
            return index_start_response_from_task(&store, status, task, requested_ref, &context)
                .await;
        }
        match if request.reuse_historical {
            plan_full_index_reuse(&store, &status, &request).await?
        } else {
            FullIndexReusePlan::Full
        } {
            FullIndexReusePlan::ActiveTask(task) => {
                return index_start_response_from_task(
                    &store,
                    status,
                    *task,
                    request.repository.ref_selector,
                    &context,
                )
                .await;
            }
            FullIndexReusePlan::Incremental(incremental_request) => {
                let requested_ref = request.repository.ref_selector.clone();
                match queue_incremental_index_task(&store, &status, &incremental_request).await {
                    Ok(task) => {
                        return index_start_response_from_task(
                            &store,
                            status,
                            task,
                            requested_ref,
                            &context,
                        )
                        .await;
                    }
                    Err(error) if historical_reuse_base_became_unavailable(&error) => {}
                    Err(error) => return Err(error),
                }
            }
            FullIndexReusePlan::Full => {}
        }
        let payload_json = serde_json::to_string(&request)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        if let Some(active_task) =
            active_full_index_task_for_request(&store, &status, &request, &payload_json).await?
        {
            return index_start_response_from_task(
                &store,
                status,
                active_task,
                request.repository.ref_selector,
                &context,
            )
            .await;
        }

        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let workspace_detection = request.workspace_detection.clone();
        let workspace_detection_json = serde_json::to_string(&workspace_detection)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let resource_budget = CodeIndexResourceBudget::default();
        let plan = run_blocking_code(move || {
            prepare_full_index_plan_with_workspace_detection(
                registration,
                selector,
                resource_budget,
                &workspace_detection,
            )
        })
        .await?;
        let session = plan.session();
        let input_fingerprint = format!(
            "full:{}:{}:{}:{}",
            session.repository_id,
            session.tree_hash,
            session.source_scope,
            workspace_detection_json
        );
        if let Some(active_task) = store
            .active_code_index_task(session.repository_id.clone())
            .await
            .map_err(storage_api_error)?
            && active_task.state.is_unfinished()
            && active_task.input_fingerprint == input_fingerprint
        {
            return index_start_response_from_task(
                &store,
                status,
                active_task,
                request.repository.ref_selector,
                &context,
            )
            .await;
        }
        let task = store
            .queue_code_index_task(CodeIndexTaskSeed {
                repository_id: session.repository_id.clone(),
                alias: status.alias.clone(),
                ref_selector: request.repository.ref_selector.clone(),
                resolved_commit_sha: session.resolved_commit_sha.clone(),
                tree_hash: session.tree_hash.clone(),
                source_scope: session.source_scope.clone(),
                path_filters: session.path_filters.clone(),
                language_filters: session.language_filters.clone(),
                mode: request.mode.clone(),
                input_fingerprint,
                resource_budget: session.resource_budget,
                payload_json,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;
        index_start_response_from_task(
            &store,
            status,
            task,
            request.repository.ref_selector,
            &context,
        )
        .await
    }
}
