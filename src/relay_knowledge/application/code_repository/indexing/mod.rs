mod business_projection;
mod durable_incremental;
mod fast_path;
mod queue;
mod state;
mod task;
mod tasks;

#[cfg(test)]
#[path = "resume_tests.rs"]
mod resume_tests;

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse,
        CodeRepositoryScopePreviewResponse, RequestContext,
    },
    code::{
        CodeIndexPlan, CodeIndexPlanRecovery, build_index_snapshot_with_workspace_detection,
        prepare_full_index_plan_with_workspace_detection, preview_repository_scope,
    },
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget, CodeIndexSnapshot,
        CodeRepositorySelector, CodeRepositoryStatus,
    },
    storage::StorageError,
};

use crate::application::service::RelayKnowledgeService;

use self::{
    business_projection::refresh_business_projection,
    durable_incremental::{
        IncrementalSnapshotApply, checkpoint_skips_parser,
        resume_finalization as resume_durable_incremental_finalization, should_resume_staged_full,
    },
    fast_path::{fresh_full_index_response, published_task_response},
    queue::{
        index_start_response_from_task, queue_incremental_index_task,
        queue_worktree_overlay_index_task,
    },
    state::{
        FullIndexReusePlan, RETAIN_RECENT_CODE_SCOPES, active_full_index_task_for_request,
        historical_reuse_base_became_unavailable, index_start_from_completed,
        plan_full_index_reuse, previous_index_state_for_index, requested_index_ref_for_response,
    },
    task::{
        CODE_INDEX_TASK_LEASE_MS, CODE_INDEX_TASK_MAX_ATTEMPTS, CODE_INDEX_TASK_RETRY_BACKOFF_MS,
        CodeIndexTaskLeaseContext, await_with_code_index_task_lease,
        code_index_task_failure_disposition, code_index_worker_lease_owner,
        finalize_code_index_session_with_task_lease, recover_orphaned_code_index_task_leases,
        refresh_code_index_task_lease,
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

fn incremental_snapshot_matches_lease(
    mode: &CodeIndexMode,
    snapshot: &CodeIndexSnapshot,
    lease: Option<&CodeIndexTaskLeaseContext>,
) -> bool {
    let Some(lease) = lease.filter(|_| matches!(mode, CodeIndexMode::Incremental { .. })) else {
        return false;
    };
    snapshot.repository_id == lease.publication_fence.repository_id
        && snapshot.source_scope == lease.source_scope
        && snapshot.resolved_commit_sha == lease.resolved_commit_sha
        && snapshot.tree_hash == lease.tree_hash
        && snapshot.path_filters == lease.path_filters
        && snapshot.language_filters == lease.language_filters
}

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
        if let Some(lease) = task_lease.as_ref() {
            let reconciled = store
                .reconcile_code_index_publication_with_fence(
                    crate::storage::CodeIndexPublicationTarget {
                        task_id: lease.task_id.clone(),
                        repository_id: lease.publication_fence.repository_id.clone(),
                        source_scope: lease.source_scope.clone(),
                        resolved_commit_sha: lease.resolved_commit_sha.clone(),
                        tree_hash: lease.tree_hash.clone(),
                        path_filters: lease.path_filters.clone(),
                        language_filters: lease.language_filters.clone(),
                    },
                    lease.publication_fence.clone(),
                )
                .await
                .map_err(storage_api_error)?;
            if reconciled {
                return published_task_response(&store, &status, &request, &context, lease).await;
            }
        }
        let requested_ref = requested_index_ref_for_response(&request);
        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let staged_checkpoint = match task_lease.as_ref() {
            Some(lease) if matches!(&request.mode, CodeIndexMode::Incremental { .. }) => store
                .code_index_checkpoint(lease.source_scope.clone())
                .await
                .map_err(storage_api_error)?,
            _ => None,
        };
        let resume_staged_full = should_resume_staged_full(
            &request.mode,
            task_lease.is_some(),
            staged_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.state.as_str()),
        );
        let resumed_incremental = resume_durable_incremental_finalization(
            &store,
            task_lease.as_ref(),
            staged_checkpoint.as_ref(),
        )
        .await?;
        let summary = if let Some(summary) = resumed_incremental {
            summary
        } else if request.mode == CodeIndexMode::Full || resume_staged_full {
            let resource_budget = task_lease
                .as_ref()
                .map(|lease| lease.resource_budget)
                .unwrap_or_default();
            self.apply_full_code_index(
                &store,
                registration.clone(),
                selector,
                request.workspace_detection.clone(),
                resource_budget,
                task_lease.clone(),
            )
            .await?
        } else {
            let previous = previous_index_state_for_index(&store, &status, &request).await?;
            let mode = request.mode.clone();
            let workspace_detection = request.workspace_detection.clone();
            let snapshot_registration = registration.clone();
            let snapshot_selector = selector.clone();
            let snapshot = await_with_code_index_task_lease(
                &store,
                task_lease.as_ref(),
                run_blocking_code(move || {
                    build_index_snapshot_with_workspace_detection(
                        &snapshot_registration,
                        &snapshot_selector,
                        mode,
                        previous.fingerprints,
                        previous.base_resolved_commit_sha,
                        &workspace_detection,
                    )
                }),
            )
            .await?;
            if let Some(lease) = task_lease.as_ref() {
                let actual_target = crate::storage::CodeIndexPublicationTarget {
                    task_id: lease.task_id.clone(),
                    repository_id: snapshot.repository_id.clone(),
                    source_scope: snapshot.source_scope.clone(),
                    resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
                    tree_hash: snapshot.tree_hash.clone(),
                    path_filters: snapshot.path_filters.clone(),
                    language_filters: snapshot.language_filters.clone(),
                };
                if store
                    .reconcile_code_index_publication_with_fence(
                        actual_target.clone(),
                        lease.publication_fence.clone(),
                    )
                    .await
                    .map_err(storage_api_error)?
                {
                    let actual_lease = CodeIndexTaskLeaseContext {
                        source_scope: actual_target.source_scope,
                        resolved_commit_sha: actual_target.resolved_commit_sha,
                        tree_hash: actual_target.tree_hash,
                        path_filters: actual_target.path_filters,
                        language_filters: actual_target.language_filters,
                        ..lease.clone()
                    };
                    return published_task_response(
                        &store,
                        &status,
                        &request,
                        &context,
                        &actual_lease,
                    )
                    .await;
                }
            }
            let durable_fallback_allowed =
                incremental_snapshot_matches_lease(&request.mode, &snapshot, task_lease.as_ref());
            let mut previous_clone_step = None;
            let mut clone_attempt_count = 0usize;
            let direct_summary = loop {
                let attempt_snapshot = snapshot.clone();
                let advance =
                    await_with_code_index_task_lease(&store, task_lease.as_ref(), async {
                        let result = match task_lease.as_ref() {
                            Some(lease) => {
                                store
                                    .apply_code_index_snapshot_with_fence(
                                        attempt_snapshot,
                                        lease.publication_fence.clone(),
                                    )
                                    .await
                            }
                            None => store.apply_code_index_snapshot(attempt_snapshot).await,
                        };
                        match result {
                            Ok(summary) => {
                                Ok(IncrementalSnapshotApply::Complete(Box::new(summary)))
                            }
                            Err(StorageError::DurableStagingPending {
                                completed_steps,
                                max_steps,
                            }) => Ok(IncrementalSnapshotApply::DurablePending {
                                completed_steps,
                                max_steps,
                            }),
                            Err(StorageError::DurableFinalizationRequired { checkpoint_state }) => {
                                Ok(IncrementalSnapshotApply::FinalizationRequired {
                                    checkpoint_state,
                                })
                            }
                            Err(StorageError::DurableStagingRequired(_))
                                if durable_fallback_allowed =>
                            {
                                Ok(IncrementalSnapshotApply::FullFallback)
                            }
                            Err(error) => Err(storage_api_error(error)),
                        }
                    })
                    .await?;
                match advance {
                    IncrementalSnapshotApply::Complete(summary) => break Some(*summary),
                    IncrementalSnapshotApply::FullFallback => break None,
                    IncrementalSnapshotApply::FinalizationRequired { checkpoint_state } => {
                        let checkpoint = store
                            .code_index_checkpoint(snapshot.source_scope.clone())
                            .await
                            .map_err(storage_api_error)?;
                        if checkpoint
                            .as_ref()
                            .is_none_or(|checkpoint| checkpoint.state != checkpoint_state)
                        {
                            return Err(ApiError::internal(format!(
                                "durable incremental finalization handoff for scope '{}' was not committed atomically",
                                snapshot.source_scope
                            )));
                        }
                        let summary = resume_durable_incremental_finalization(
                            &store,
                            task_lease.as_ref(),
                            checkpoint.as_ref(),
                        )
                        .await?
                        .ok_or_else(|| {
                            ApiError::internal(format!(
                                "durable incremental finalization receipt for scope '{}' is missing",
                                snapshot.source_scope
                            ))
                        })?;
                        break Some(summary);
                    }
                    IncrementalSnapshotApply::DurablePending {
                        completed_steps,
                        max_steps,
                    } => {
                        clone_attempt_count = clone_attempt_count.saturating_add(1);
                        let progressed =
                            previous_clone_step.is_none_or(|previous| completed_steps > previous);
                        if max_steps == 0
                            || completed_steps > max_steps
                            || clone_attempt_count > max_steps.saturating_add(1)
                            || !progressed
                        {
                            return Err(ApiError::internal(format!(
                                "durable incremental clone for scope '{}' did not advance within its step proof",
                                snapshot.source_scope
                            )));
                        }
                        previous_clone_step = Some(completed_steps);
                    }
                }
            };
            match direct_summary {
                Some(summary) => summary,
                None => {
                    let resource_budget = task_lease
                        .as_ref()
                        .map(|lease| lease.resource_budget)
                        .unwrap_or_default();
                    self.apply_full_code_index(
                        &store,
                        registration.clone(),
                        selector,
                        request.workspace_detection.clone(),
                        resource_budget,
                        task_lease.clone(),
                    )
                    .await?
                }
            }
        };
        if let Some(lease) = task_lease.as_ref() {
            let checkpoint = store
                .code_index_checkpoint(summary.source_scope.clone())
                .await
                .map_err(storage_api_error)?;
            if let Some(checkpoint) = checkpoint.as_ref().filter(|checkpoint| {
                matches!(
                    checkpoint.state.as_str(),
                    "finalizing:partitioned_publish" | "completed"
                )
            }) {
                let reconciled = store
                    .reconcile_code_index_publication_with_fence(
                        crate::storage::CodeIndexPublicationTarget {
                            task_id: lease.task_id.clone(),
                            repository_id: summary.repository_id.clone(),
                            source_scope: summary.source_scope.clone(),
                            resolved_commit_sha: summary.resolved_commit_sha.clone(),
                            tree_hash: summary.tree_hash.clone(),
                            path_filters: lease.path_filters.clone(),
                            language_filters: lease.language_filters.clone(),
                        },
                        lease.publication_fence.clone(),
                    )
                    .await
                    .map_err(storage_api_error)?;
                if reconciled {
                    return published_task_response(&store, &status, &request, &context, lease)
                        .await;
                }
                if checkpoint.state == "finalizing:partitioned_publish" {
                    return Err(ApiError::storage_unavailable(format!(
                        "partitioned finalization for scope '{}' did not publish its catalog handoff",
                        summary.source_scope
                    )));
                }
            }
        }
        if !request.workspace_detection.enabled {
            await_with_code_index_task_lease(&store, task_lease.as_ref(), async {
                match task_lease.as_ref() {
                    Some(lease) => {
                        store
                            .clear_code_workspace_state_with_fence(
                                summary.repository_id.clone(),
                                summary.source_scope.clone(),
                                lease.publication_fence.clone(),
                            )
                            .await
                    }
                    None => {
                        store
                            .clear_code_workspace_state(
                                summary.repository_id.clone(),
                                summary.source_scope.clone(),
                            )
                            .await
                    }
                }
                .map_err(storage_api_error)
            })
            .await?;
        }
        refresh_business_projection(&store, registration, &summary, task_lease.as_ref()).await?;
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
        let status = store
            .code_repository_status(summary.repository_id.clone())
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| ApiError::storage_unavailable("code repository status is missing"))?;
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
        self.apply_code_index_from_plan(store, plan, task_lease)
            .await
    }

    /// Runs the checkpointed session lifecycle: begin, batch loop, finalize.
    ///
    /// Shared by full and incremental index paths. The plan determines
    /// whether the session is full-replace or incremental.
    async fn apply_code_index_from_plan(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
        plan: CodeIndexPlan,
        task_lease: Option<CodeIndexTaskLeaseContext>,
    ) -> Result<crate::domain::CodeIndexSummary, ApiError> {
        let session = plan.session();
        let preflight_checkpoint = store
            .code_index_checkpoint(session.source_scope.clone())
            .await
            .map_err(storage_api_error)?;
        let (plan, content_equivalent_restart) = match preflight_checkpoint.as_ref() {
            Some(checkpoint) => {
                let checkpoint = checkpoint.clone();
                match run_blocking_code(move || plan.recover_from_checkpoint(&checkpoint)).await? {
                    CodeIndexPlanRecovery::Resume(plan) => (plan, false),
                    CodeIndexPlanRecovery::ContentEquivalentRestart(plan) => (plan, true),
                }
            }
            None => (plan, false),
        };
        let checkpoint = match task_lease.as_ref() {
            Some(lease) => {
                store
                    .begin_code_index_session_at_checkpoint_with_fence(
                        session.clone(),
                        preflight_checkpoint.clone(),
                        lease.publication_fence.clone(),
                    )
                    .await
            }
            None => {
                store
                    .begin_code_index_session_at_checkpoint(
                        session.clone(),
                        preflight_checkpoint.clone(),
                    )
                    .await
            }
        }
        .map_err(storage_api_error)?;
        let plan = match preflight_checkpoint {
            Some(preflight) => {
                if content_equivalent_restart {
                    let checkpoint = checkpoint.clone();
                    run_blocking_code(move || {
                        plan.resume_from_content_equivalent_restart_checkpoint(&checkpoint)
                    })
                    .await?
                } else if checkpoint != preflight {
                    return Err(ApiError::internal(format!(
                        "code index checkpoint '{}' changed after resume preflight",
                        checkpoint.source_scope
                    )));
                } else {
                    plan
                }
            }
            None => {
                let checkpoint = checkpoint.clone();
                run_blocking_code(move || plan.resume_from_checkpoint(&checkpoint)).await?
            }
        };
        refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
        if !checkpoint_skips_parser(&checkpoint.state) {
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
            let writer_result =
                await_with_code_index_task_lease(store, task_lease.as_ref(), async {
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
        }

        let summary = match task_lease.as_ref() {
            Some(lease) => {
                finalize_code_index_session_with_task_lease(store, lease, session).await?
            }
            None => store
                .finalize_code_index_session(session)
                .await
                .map_err(storage_api_error)?,
        };

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
        index_start_response_from_task(
            &store,
            status,
            task,
            request.repository.ref_selector,
            &context,
        )
        .await
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
                    .complete_code_index_task(crate::storage::CodeIndexTaskCompletion {
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
                    .prune_code_repository_scopes(crate::storage::CodeScopeRetentionRequest {
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
                    .fail_code_index_task(crate::storage::CodeIndexTaskFailure {
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
        let path_filters = crate::application::code_repository::scope::merged_filters(
            &status.path_filters,
            &request.repository.path_filters,
        );
        let language_filters = crate::application::code_repository::scope::merged_filters(
            &status.language_filters,
            &request.repository.language_filters,
        );
        let scope_id = crate::domain::code_snapshot_scope_id_with_workspace_detection(
            &preview.repository_id,
            &preview.tree_hash,
            &path_filters,
            &language_filters,
            &request.workspace_detection,
        );
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        Ok(CodeRepositoryScopePreviewResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata {
                scope_id: scope_id.clone(),
                repository_id: preview.repository_id.clone(),
                alias: preview.alias.clone(),
                requested_ref: request.repository.ref_selector,
                resolved_commit_sha: preview.resolved_commit_sha.clone(),
                tree_hash: preview.tree_hash.clone(),
                path_filters,
                language_filters,
                indexed_file_count: preview.selected_file_count,
                index_versions: vec![format!("code:{scope_id}:{}", preview.tree_hash)],
                stale: true,
            },
            preview,
        })
    }
}
