mod fast_path;
mod queue;
mod state;
mod task;
mod tasks;

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse,
        CodeRepositoryScopePreviewResponse, RequestContext,
    },
    code::{
        build_index_snapshot_with_workspace_detection,
        prepare_full_index_plan_with_workspace_detection, preview_repository_scope,
    },
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget, CodeRepositorySelector,
        CodeRepositoryStatus,
    },
};

use crate::application::service::RelayKnowledgeService;

use self::{
    fast_path::fresh_full_index_response,
    queue::{queue_incremental_index_task, queue_worktree_overlay_index_task},
    state::{
        RETAIN_RECENT_CODE_SCOPES, active_full_index_task_for_request, index_start_from_completed,
        previous_index_state_for_index, requested_index_ref_for_response,
    },
    task::{
        CODE_INDEX_TASK_LEASE_MS, CODE_INDEX_TASK_MAX_ATTEMPTS, CODE_INDEX_TASK_RETRY_BACKOFF_MS,
        CodeIndexTaskLeaseContext, await_with_code_index_task_lease, code_index_worker_lease_owner,
        recover_orphaned_code_index_task_leases, refresh_code_index_task_lease,
    },
};
use super::{
    blocking::run_blocking_code,
    clock::now_millis,
    errors::storage_api_error,
    repository::{registration_from_status, required_code_repository},
    worktree_ref::pending_worktree_overlay_base_commit,
};

pub(super) use task::recover_code_index_task_leases;

const PARSED_BATCH_QUEUE_CAPACITY: usize = 2;

impl RelayKnowledgeService {
    /// Builds or updates the tree-sitter code index for a registered repository.
    pub async fn index_code_repository(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexResponse, ApiError> {
        let started = self
            .start_code_repository_index(request, context.clone())
            .await?;
        if let Some(summary) = started.summary {
            return Ok(CodeRepositoryIndexResponse {
                metadata: started.metadata,
                scope: started.scope,
                summary,
                status: started.status,
            });
        }
        let task_id = started
            .task
            .as_ref()
            .map(|task| task.task_id.clone())
            .ok_or_else(|| {
                ApiError::storage_unavailable("durable repository index did not return a task")
            })?;
        self.run_code_index_task_once_with_response(Some(task_id.clone()), context)
            .await?
            .map(|(_, response)| response)
            .ok_or_else(|| {
                ApiError::qos_rejected(format!(
                    "durable repository index task '{task_id}' is already claimed or queued behind another repository writer; inspect repo status and let the managed worker drain it"
                ))
            })
    }

    async fn index_code_repository_inner(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
        task_lease: Option<CodeIndexTaskLeaseContext>,
    ) -> Result<CodeRepositoryIndexResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if let Some(response) = fresh_full_index_response(
            &store,
            &status,
            &request,
            &context,
            task_lease.as_ref().map(|lease| &lease.publication_fence),
        )
        .await?
        {
            return Ok(response);
        }
        let requested_ref = requested_index_ref_for_response(&request);
        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let summary = if request.mode == CodeIndexMode::Full {
            self.apply_full_code_index(
                &store,
                registration,
                selector,
                request.workspace_detection.clone(),
                CodeIndexResourceBudget::default(),
                task_lease.clone(),
            )
            .await?
        } else {
            let previous = previous_index_state_for_index(&store, &status, &request).await?;
            let mode = request.mode;
            let workspace_detection = request.workspace_detection.clone();
            let snapshot = await_with_code_index_task_lease(
                &store,
                task_lease.as_ref(),
                run_blocking_code(move || {
                    build_index_snapshot_with_workspace_detection(
                        &registration,
                        &selector,
                        mode,
                        previous.fingerprints,
                        previous.base_resolved_commit_sha,
                        &workspace_detection,
                    )
                }),
            )
            .await?;
            await_with_code_index_task_lease(&store, task_lease.as_ref(), async {
                match task_lease.as_ref() {
                    Some(lease) => {
                        store
                            .apply_code_index_snapshot_with_fence(
                                snapshot,
                                lease.publication_fence.clone(),
                            )
                            .await
                    }
                    None => store.apply_code_index_snapshot(snapshot).await,
                }
                .map_err(storage_api_error)
            })
            .await?
        };
        refresh_code_index_task_lease(&store, task_lease.as_ref()).await?;
        let status = store
            .code_repository_status(summary.repository_id.clone())
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| ApiError::storage_unavailable("code repository status is missing"))?;
        let software_projection =
            await_with_code_index_task_lease(&store, task_lease.as_ref(), async {
                match task_lease.as_ref() {
                    Some(lease) => {
                        store
                            .refresh_software_global_projection_with_fence(
                                summary.source_scope.clone(),
                                lease.publication_fence.clone(),
                            )
                            .await
                    }
                    None => {
                        store
                            .refresh_software_global_projection(summary.source_scope.clone())
                            .await
                    }
                }
                .map_err(storage_api_error)
            })
            .await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let degraded_reason = status
            .degraded_reason
            .clone()
            .or(software_projection.status.last_error.clone());
        let status = CodeRepositoryStatus {
            degraded_reason,
            ..status
        };
        let _ = self.refresh_watched_code_repository(&status).await;

        Ok(CodeRepositoryIndexResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                &status,
                &request.repository,
                requested_ref,
            ),
            summary,
            status,
        })
    }

    async fn apply_full_code_index(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
        registration: crate::domain::CodeRepositoryRegistration,
        selector: CodeRepositorySelector,
        workspace_detection: crate::domain::CodeWorkspaceDetectionConfig,
        resource_budget: CodeIndexResourceBudget,
        task_lease: Option<CodeIndexTaskLeaseContext>,
    ) -> Result<crate::domain::CodeIndexSummary, ApiError> {
        let plan = await_with_code_index_task_lease(
            store,
            task_lease.as_ref(),
            run_blocking_code(move || {
                prepare_full_index_plan_with_workspace_detection(
                    registration,
                    selector,
                    resource_budget,
                    &workspace_detection,
                )
            }),
        )
        .await?;
        let session = plan.session();
        match task_lease.as_ref() {
            Some(lease) => {
                store
                    .begin_code_index_session_with_fence(
                        session.clone(),
                        lease.publication_fence.clone(),
                    )
                    .await
            }
            None => store.begin_code_index_session(session.clone()).await,
        }
        .map_err(storage_api_error)?;
        refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
        let (batch_sender, mut batch_receiver) =
            tokio::sync::mpsc::channel(PARSED_BATCH_QUEUE_CAPACITY);
        let parser = tokio::spawn(run_blocking_code(move || {
            let mut plan = plan;
            loop {
                let (next_plan, batch) = plan.parse_next_batch()?;
                plan = next_plan;
                let Some(batch) = batch else {
                    return Ok(());
                };
                if batch_sender.blocking_send(batch).is_err() {
                    return Ok(());
                }
            }
        }));
        let writer_result = await_with_code_index_task_lease(store, task_lease.as_ref(), async {
            while let Some(batch) = batch_receiver.recv().await {
                refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
                match task_lease.as_ref() {
                    Some(lease) => {
                        store
                            .apply_code_index_batch_with_fence(
                                batch,
                                lease.publication_fence.clone(),
                            )
                            .await
                    }
                    None => store.apply_code_index_batch(batch).await,
                }
                .map_err(storage_api_error)?;
                refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
            }
            Ok::<(), ApiError>(())
        })
        .await;
        drop(batch_receiver);
        let parser_result = parser
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
        writer_result?;
        parser_result?;

        let summary = await_with_code_index_task_lease(store, task_lease.as_ref(), async {
            match task_lease.as_ref() {
                Some(lease) => {
                    store
                        .finalize_code_index_session_with_fence(
                            session,
                            lease.publication_fence.clone(),
                        )
                        .await
                }
                None => store.finalize_code_index_session(session).await,
            }
            .map_err(storage_api_error)
        })
        .await?;

        Ok(summary)
    }

    /// Starts a repository index request, queueing cold full indexes for background execution.
    pub async fn start_code_repository_index(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexStartResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if let Some(response) =
            fresh_full_index_response(&store, &status, &request, &context, None).await?
        {
            return Ok(index_start_from_completed(response, None));
        }
        recover_code_index_task_leases(&store, now_millis()).await?;
        if matches!(request.mode, CodeIndexMode::Incremental { .. }) {
            let requested_ref = requested_index_ref_for_response(&request);
            let task = queue_incremental_index_task(&store, &status, &request).await?;
            return self
                .index_start_response_from_task(&store, status, task, requested_ref, &context)
                .await;
        }
        if request.mode == CodeIndexMode::WorktreeOverlay {
            let requested_ref = requested_index_ref_for_response(&request);
            let task = queue_worktree_overlay_index_task(&store, &status, &request).await?;
            return self
                .index_start_response_from_task(&store, status, task, requested_ref, &context)
                .await;
        }
        let payload_json = serde_json::to_string(&request)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        if let Some(active_task) =
            active_full_index_task_for_request(&store, &status, &request, &payload_json).await?
        {
            return self
                .index_start_response_from_task(
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
            return self
                .index_start_response_from_task(
                    &store,
                    status,
                    active_task,
                    request.repository.ref_selector,
                    &context,
                )
                .await;
        }
        let task = store
            .queue_code_index_task(crate::storage::CodeIndexTaskSeed {
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
        self.index_start_response_from_task(
            &store,
            status,
            task,
            request.repository.ref_selector,
            &context,
        )
        .await
    }

    async fn index_start_response_from_task(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
        fallback_status: CodeRepositoryStatus,
        task: crate::domain::CodeIndexTaskRecord,
        requested_ref: String,
        context: &RequestContext,
    ) -> Result<CodeRepositoryIndexStartResponse, ApiError> {
        let checkpoint = store
            .code_index_checkpoint(task.source_scope.clone())
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let status = store
            .code_repository_status(task.repository_id.clone())
            .await
            .map_err(storage_api_error)?
            .unwrap_or(fallback_status);

        Ok(CodeRepositoryIndexStartResponse {
            metadata: ApiMetadata::graph_only(context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_index_task(&task, requested_ref),
            summary: None,
            status,
            task: Some(task),
            checkpoint,
        })
    }

    /// Runs one queued code index task under a lease.
    pub async fn run_code_index_task_once(
        &self,
        task_id: Option<String>,
        context: RequestContext,
    ) -> Result<Option<crate::domain::CodeIndexTaskRecord>, ApiError> {
        self.run_code_index_task_once_with_response(task_id, context)
            .await
            .map(|outcome| outcome.map(|(task, _)| task))
    }

    pub(crate) async fn run_code_index_task_once_with_response(
        &self,
        task_id: Option<String>,
        context: RequestContext,
    ) -> Result<
        Option<(
            crate::domain::CodeIndexTaskRecord,
            CodeRepositoryIndexResponse,
        )>,
        ApiError,
    > {
        let store = self.store().await.map_err(storage_api_error)?;
        let lease_owner = code_index_worker_lease_owner();
        let Some(task) = store
            .claim_code_index_task(crate::storage::CodeIndexTaskClaimRequest {
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
                    .fail_code_index_task(crate::storage::CodeIndexTaskFailure {
                        task_id: task.task_id,
                        lease_owner,
                        attempt_count: task.attempt_count,
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
        } else {
            request.repository.ref_selector = task.resolved_commit_sha.clone();
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
        };
        let result = self
            .index_code_repository_inner(request, context, Some(lease_context.clone()))
            .await;
        match result {
            Ok(response) => {
                refresh_code_index_task_lease(&store, Some(&lease_context)).await?;
                let completed = store
                    .complete_code_index_task(crate::storage::CodeIndexTaskCompletion {
                        task_id: task.task_id.clone(),
                        lease_owner,
                        attempt_count: task.attempt_count,
                        now_ms: now_millis(),
                    })
                    .await
                    .map_err(storage_api_error)?;
                if let Err(error) = store
                    .schedule_code_repository_retention(
                        self.runtime.workers.code_index_max_indexed_repositories,
                        now_millis(),
                    )
                    .await
                {
                    tracing::warn!(
                        task_id = %completed.task_id,
                        error = %error,
                        "code index published but repository retention was not scheduled"
                    );
                }
                if let Err(error) = store
                    .prune_code_repository_scopes(crate::storage::CodeScopeRetentionRequest {
                        repository_id: response.summary.repository_id.clone(),
                        active_scope: response.summary.source_scope.clone(),
                        retain_recent_successful_scopes: RETAIN_RECENT_CODE_SCOPES,
                        repository_retention_cutoff_ms: None,
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
                let _ = store
                    .fail_code_index_task(crate::storage::CodeIndexTaskFailure {
                        task_id: task.task_id,
                        lease_owner,
                        attempt_count: task.attempt_count,
                        error_kind: "code_index".to_owned(),
                        error_message: error.message.clone(),
                        retry_backoff_ms: CODE_INDEX_TASK_RETRY_BACKOFF_MS,
                        max_attempts: CODE_INDEX_TASK_MAX_ATTEMPTS,
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

    /// Previews the effective code repository indexing scope without writing rows.
    pub async fn preview_code_repository_scope(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryScopePreviewResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let preview =
            run_blocking_code(move || preview_repository_scope(&registration, &selector)).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        Ok(CodeRepositoryScopePreviewResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                &status,
                &request.repository,
                request.repository.ref_selector.clone(),
            ),
            preview,
        })
    }
}
