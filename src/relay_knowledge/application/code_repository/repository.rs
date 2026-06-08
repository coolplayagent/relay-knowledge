use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryFeatureFlagsResponse, CodeRepositoryImpactResponse,
        CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse, CodeRepositoryQueryResponse,
        CodeRepositoryRegisterRequest, CodeRepositoryRegisterResponse,
        CodeRepositoryRemoveResponse, CodeRepositoryReportResponse,
        CodeRepositoryScopePreviewResponse, CodeRepositoryStatusResponse, RequestContext,
    },
    code::{
        REGISTRATION_LANGUAGE_FILTER_ERROR, build_index_snapshot_with_base_commit,
        changed_paths_for_diff_with_filters, changed_paths_for_filesystem_diff,
        deleted_symbol_names_for_diff, partition_changed_paths_for_selector,
        prepare_full_index_plan, preview_repository_scope, register_repository,
    },
    domain::{
        CodeFeatureFlagRequest, CodeImpactRequest, CodeIndexMode, CodeIndexRequest,
        CodeIndexResourceBudget, CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalHit,
        CodeRetrievalRequest, FreshnessPolicy, StalenessHint,
    },
    storage::CodeImpactChanges,
};

use crate::application::service::RelayKnowledgeService;

use super::fast_index::fresh_full_index_response;
use super::freshness::{
    CodeFeatureFlagFreshnessContext, CodeQueryFreshnessContext,
    code_feature_flag_freshness_diagnostics, code_query_freshness_diagnostics,
};
use super::support::{
    CODE_INDEX_TASK_LEASE_MS, CODE_INDEX_TASK_MAX_ATTEMPTS, CODE_INDEX_TASK_RETRY_BACKOFF_MS,
    CodeIndexTaskLeaseContext, RETAIN_RECENT_CODE_SCOPES, active_index_matches_request,
    apply_code_grep_fallback, code_index_worker_lease_owner, code_status_checkpoint,
    feature_flag_request_at_indexed_ref, index_start_from_completed, indexed_source_scope,
    latest_compatible_code_scope_status, missing_indexed_source_scope_error, now_millis,
    previous_index_state_for_index, recover_code_index_task_leases,
    recover_orphaned_code_index_task_leases, refresh_code_index_task_lease,
    registration_from_status, required_code_repository, resolve_code_ref_for_selector,
    resolved_code_scope_status, retrieval_request_at_indexed_ref, run_blocking_code,
    storage_api_error,
};

impl RelayKnowledgeService {
    /// Registers a Git repository as a code source.
    pub async fn register_code_repository(
        &self,
        request: CodeRepositoryRegisterRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryRegisterResponse, ApiError> {
        if !request.language_filters.is_empty() {
            return Err(ApiError::invalid_argument(
                REGISTRATION_LANGUAGE_FILTER_ERROR,
            ));
        }
        let registration = run_blocking_code(move || {
            register_repository(
                request.root_path,
                request.alias,
                request.path_filters,
                request.language_filters,
            )
        })
        .await?;
        let store = self.store().await.map_err(storage_api_error)?;
        let status = store
            .upsert_code_repository(registration.clone())
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositoryRegisterResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            registration,
            status,
        })
    }

    /// Removes a registered code repository and its derived index state.
    pub async fn remove_code_repository(
        &self,
        repository: String,
        context: RequestContext,
    ) -> Result<CodeRepositoryRemoveResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let now_ms = now_millis();
        recover_code_index_task_leases(&store, now_ms).await?;
        let removed_status = required_code_repository(&store, &repository).await?;
        let summary = store
            .remove_code_repository(removed_status.repository_id.clone(), now_ms)
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| {
                ApiError::storage_unavailable("removed code repository disappeared before delete")
            })?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositoryRemoveResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            removed_status,
            summary,
        })
    }

    /// Builds or updates the tree-sitter code index for a registered repository.
    pub async fn index_code_repository(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexResponse, ApiError> {
        self.index_code_repository_inner(request, context, None)
            .await
    }

    async fn index_code_repository_inner(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
        task_lease: Option<CodeIndexTaskLeaseContext>,
    ) -> Result<CodeRepositoryIndexResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if let Some(response) =
            fresh_full_index_response(&store, &status, &request, &context).await?
        {
            return Ok(response);
        }
        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let summary = if request.mode == CodeIndexMode::Full {
            self.apply_full_code_index(
                &store,
                registration,
                selector,
                CodeIndexResourceBudget::default(),
                task_lease,
            )
            .await?
        } else {
            let previous = previous_index_state_for_index(&store, &status, &request).await?;
            let mode = request.mode;
            let snapshot = run_blocking_code(move || {
                build_index_snapshot_with_base_commit(
                    &registration,
                    &selector,
                    mode,
                    previous.fingerprints,
                    previous.base_resolved_commit_sha,
                )
            })
            .await?;
            store
                .apply_code_index_snapshot(snapshot)
                .await
                .map_err(storage_api_error)?
        };
        let status = store
            .code_repository_status(summary.repository_id.clone())
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| ApiError::storage_unavailable("code repository status is missing"))?;
        let software_projection = store
            .refresh_software_global_projection(summary.source_scope.clone())
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositoryIndexResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                &status,
                &request.repository,
                request.repository.ref_selector.clone(),
            ),
            summary,
            status: CodeRepositoryStatus {
                degraded_reason: status
                    .degraded_reason
                    .or(software_projection.status.last_error.clone()),
                ..status
            },
        })
    }

    async fn apply_full_code_index(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
        registration: crate::domain::CodeRepositoryRegistration,
        selector: CodeRepositorySelector,
        resource_budget: CodeIndexResourceBudget,
        task_lease: Option<CodeIndexTaskLeaseContext>,
    ) -> Result<crate::domain::CodeIndexSummary, ApiError> {
        let mut plan = run_blocking_code(move || {
            prepare_full_index_plan(registration, selector, resource_budget)
        })
        .await?;
        let session = plan.session();
        store
            .begin_code_index_session(session.clone())
            .await
            .map_err(storage_api_error)?;
        loop {
            refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
            let (next_plan, batch) = run_blocking_code(move || plan.parse_next_batch()).await?;
            plan = next_plan;
            let Some(batch) = batch else {
                break;
            };
            store
                .apply_code_index_batch(batch)
                .await
                .map_err(storage_api_error)?;
            refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
        }

        refresh_code_index_task_lease(store, task_lease.as_ref()).await?;
        let summary = store
            .finalize_code_index_session(session)
            .await
            .map_err(storage_api_error)?;
        refresh_code_index_task_lease(store, task_lease.as_ref()).await?;

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
        if request.mode != CodeIndexMode::Full {
            let response = self.index_code_repository(request, context).await?;
            return Ok(index_start_from_completed(response, None));
        }
        recover_code_index_task_leases(&store, now_millis()).await?;
        let payload_json = serde_json::to_string(&request)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        if let Some(active_task) = self
            .active_full_index_task_for_request(&store, &status, &request, &payload_json)
            .await?
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
        let resource_budget = CodeIndexResourceBudget::default();
        let plan = run_blocking_code(move || {
            prepare_full_index_plan(registration, selector, resource_budget)
        })
        .await?;
        let session = plan.session();
        let input_fingerprint = format!(
            "full:{}:{}:{}",
            session.repository_id, session.tree_hash, session.source_scope
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

    async fn active_full_index_task_for_request(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
        status: &CodeRepositoryStatus,
        request: &CodeIndexRequest,
        payload_json: &str,
    ) -> Result<Option<crate::domain::CodeIndexTaskRecord>, ApiError> {
        let Some(active_task) = store
            .active_code_index_task(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?
        else {
            return Ok(None);
        };
        if !active_task.state.is_unfinished()
            || active_task.mode != CodeIndexMode::Full
            || active_task.payload_json != payload_json
        {
            return Ok(None);
        }
        let resolved = resolve_code_ref_for_selector(
            status,
            &request.repository,
            request.repository.ref_selector.clone(),
        )
        .await?;
        if resolved == active_task.resolved_commit_sha {
            Ok(Some(active_task))
        } else {
            Ok(None)
        }
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
        request.repository.ref_selector = task.resolved_commit_sha.clone();
        let lease_context = CodeIndexTaskLeaseContext {
            task_id: task.task_id.clone(),
            lease_owner: lease_owner.clone(),
            attempt_count: task.attempt_count,
            lease_duration_ms: CODE_INDEX_TASK_LEASE_MS,
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
                let _ = store
                    .prune_code_repository_scopes(crate::storage::CodeScopeRetentionRequest {
                        repository_id: response.summary.repository_id,
                        active_scope: response.summary.source_scope,
                        retain_recent_successful_scopes: RETAIN_RECENT_CODE_SCOPES,
                    })
                    .await;
                Ok(Some(completed))
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
        recover_orphaned_code_index_task_leases(&store, now_millis()).await
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

    /// Queries indexed symbols, references, imports, calls, and code chunks.
    pub async fn query_code_repository(
        &self,
        request: CodeRetrievalRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryQueryResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let graph_version = store
                .current_graph_version()
                .await
                .map_err(storage_api_error)?;
            let degraded_reason = "graph_only freshness policy selected".to_owned();
            return Ok(CodeRepositoryQueryResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                    &status,
                    &request.repository,
                    request.repository.ref_selector.clone(),
                ),
                freshness: crate::api::CodeRepositoryFreshnessDiagnostics::graph_only(
                    graph_version.get(),
                    request.freshness_policy,
                    indexed_source_scope(&status),
                    request.repository.ref_selector.clone(),
                    degraded_reason.clone(),
                ),
                request,
                results: Vec::new(),
                degraded_reason: Some(degraded_reason),
            });
        }
        let requested_ref = request.repository.ref_selector.clone();
        let mut request = retrieval_request_at_indexed_ref(request, &status).await?;
        let requested_resolved_ref = request.repository.ref_selector.clone();
        let freshness_target = request.repository.clone();
        let mut served_stale_scope = false;
        let mut stale_reason = None;
        let scoped_status = match resolved_code_scope_status(&store, &status, &request.repository)
            .await
        {
            Ok(scoped_status) => scoped_status,
            Err(error) if request.freshness_policy == FreshnessPolicy::AllowStale => {
                if !active_index_matches_request(&store, &status, &request.repository).await? {
                    return Err(error);
                }
                let Some(stale_status) =
                    latest_compatible_code_scope_status(&store, &request.repository).await?
                else {
                    return Err(error);
                };
                let Some(last_indexed_commit) = stale_status.last_indexed_commit.clone() else {
                    return Err(error);
                };
                request.repository.ref_selector = last_indexed_commit;
                served_stale_scope = true;
                stale_reason = Some(
                    "requested ref is not indexed yet; served last completed code index".to_owned(),
                );
                stale_status
            }
            Err(error) => return Err(error),
        };
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh && scoped_status.stale {
            return Err(ApiError::invalid_argument(format!(
                "code repository '{}' scope '{}' is stale; run repo index or repo update before querying with wait_until_fresh",
                scoped_status.alias,
                scoped_status
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped")
            )));
        }
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let source_scope = indexed_source_scope(&scoped_status)
            .ok_or_else(|| missing_indexed_source_scope_error(&scoped_status))?;
        let mut results = store
            .search_code_scope(source_scope, request.clone())
            .await
            .map_err(storage_api_error)?;
        let fallback_degraded_reason =
            apply_code_grep_fallback(&store, &status, &scoped_status, &request, &mut results)
                .await?;
        let degraded_reason = results
            .iter()
            .find_map(|hit| hit.degraded_reason.clone())
            .or(fallback_degraded_reason)
            .or_else(|| scoped_status.degraded_reason.clone())
            .or_else(|| stale_reason.clone());
        let mut scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            requested_ref.clone(),
        );
        if served_stale_scope {
            scope.stale = true;
        }
        let mut metadata = ApiMetadata::graph_only(&context, graph_version);
        if served_stale_scope {
            metadata.stale = true;
        }
        let freshness = code_query_freshness_diagnostics(
            &store,
            CodeQueryFreshnessContext {
                base_status: &status,
                scoped_status: &scoped_status,
                request: &request,
                requested_ref,
                requested_resolved_ref,
                freshness_target,
                stale_reason,
                degraded_reason: degraded_reason.clone(),
                results: &results,
                graph_version: graph_version.get(),
            },
        )
        .await?;
        annotate_query_result_staleness(&mut results, &freshness);

        Ok(CodeRepositoryQueryResponse {
            metadata,
            scope,
            freshness,
            request,
            results,
            degraded_reason,
        })
    }

    /// Lists configuration-driven feature flags and their code graph relationships.
    pub async fn query_code_repository_feature_flags(
        &self,
        request: CodeFeatureFlagRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryFeatureFlagsResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if request.freshness_policy == FreshnessPolicy::GraphOnly {
            let graph_version = store
                .current_graph_version()
                .await
                .map_err(storage_api_error)?;
            let degraded_reason = "graph_only freshness policy selected".to_owned();
            return Ok(CodeRepositoryFeatureFlagsResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                    &status,
                    &request.repository,
                    request.repository.ref_selector.clone(),
                ),
                freshness: crate::api::CodeRepositoryFreshnessDiagnostics::graph_only(
                    graph_version.get(),
                    request.freshness_policy,
                    indexed_source_scope(&status),
                    request.repository.ref_selector.clone(),
                    degraded_reason.clone(),
                ),
                request,
                flags: Vec::new(),
                degraded_reason: Some(degraded_reason),
            });
        }
        let requested_ref = request.repository.ref_selector.clone();
        let mut request = feature_flag_request_at_indexed_ref(request, &status).await?;
        let requested_resolved_ref = request.repository.ref_selector.clone();
        let freshness_target = request.repository.clone();
        let mut served_stale_scope = false;
        let mut stale_reason = None;
        let scoped_status = match resolved_code_scope_status(&store, &status, &request.repository)
            .await
        {
            Ok(scoped_status) => scoped_status,
            Err(error) if request.freshness_policy == FreshnessPolicy::AllowStale => {
                if !active_index_matches_request(&store, &status, &request.repository).await? {
                    return Err(error);
                }
                let Some(stale_status) =
                    latest_compatible_code_scope_status(&store, &request.repository).await?
                else {
                    return Err(error);
                };
                let Some(last_indexed_commit) = stale_status.last_indexed_commit.clone() else {
                    return Err(error);
                };
                request.repository.ref_selector = last_indexed_commit;
                served_stale_scope = true;
                stale_reason = Some(
                    "requested ref is not indexed yet; served last completed code index".to_owned(),
                );
                stale_status
            }
            Err(error) => return Err(error),
        };
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh && scoped_status.stale {
            return Err(ApiError::invalid_argument(format!(
                "code repository '{}' scope '{}' is stale; run repo index or repo update before querying feature flags with wait_until_fresh",
                scoped_status.alias,
                scoped_status
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped")
            )));
        }
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let source_scope = indexed_source_scope(&scoped_status)
            .ok_or_else(|| missing_indexed_source_scope_error(&scoped_status))?;
        let flags = store
            .search_code_feature_flags_scope(source_scope, request.clone())
            .await
            .map_err(storage_api_error)?;
        let mut scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            requested_ref.clone(),
        );
        if served_stale_scope {
            scope.stale = true;
        }
        let degraded_reason = scoped_status
            .degraded_reason
            .clone()
            .or_else(|| stale_reason.clone());
        let mut metadata = ApiMetadata::graph_only(&context, graph_version);
        if served_stale_scope {
            metadata.stale = true;
        }
        let freshness = code_feature_flag_freshness_diagnostics(
            &store,
            CodeFeatureFlagFreshnessContext {
                base_status: &status,
                scoped_status: &scoped_status,
                request: &request,
                requested_ref,
                requested_resolved_ref,
                freshness_target,
                stale_reason,
                degraded_reason: degraded_reason.clone(),
                flags: &flags,
                graph_version: graph_version.get(),
            },
        )
        .await?;

        Ok(CodeRepositoryFeatureFlagsResponse {
            metadata,
            scope,
            freshness,
            request,
            flags,
            degraded_reason,
        })
    }

    /// Returns impact radius for a Git diff using the indexed code graph.
    pub async fn impact_code_repository(
        &self,
        mut request: CodeImpactRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryImpactResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        let head_commit =
            resolve_code_ref_for_selector(&status, &request.repository, request.head_ref.clone())
                .await?;
        request.repository.ref_selector = head_commit.clone();
        let scoped_status =
            resolved_code_scope_status(&store, &status, &request.repository).await?;
        let root = PathBuf::from(status.root_path.clone());
        let base_ref = request.base_ref.clone();
        let head_ref = head_commit.clone();
        let path_filters = scoped_status.path_filters.clone();
        let language_filters = scoped_status.language_filters.clone();
        let base_fingerprints = if base_ref.starts_with("filesystem:") {
            let mut base_selector = request.repository.clone();
            base_selector.ref_selector = base_ref.clone();
            match resolved_code_scope_status(&store, &status, &base_selector).await {
                Ok(base_status) => match base_status.last_indexed_scope_id {
                    Some(source_scope) => Some(
                        store
                            .code_file_fingerprints_for_scope(source_scope)
                            .await
                            .map_err(storage_api_error)?,
                    ),
                    None => None,
                },
                Err(_) => None,
            }
        } else {
            None
        };
        let changed_paths = if let Some(base_fingerprints) = base_fingerprints {
            run_blocking_code(move || {
                let previous_hashes = base_fingerprints
                    .into_iter()
                    .map(|file| (file.path, file.blob_hash))
                    .collect::<BTreeMap<_, _>>();
                changed_paths_for_filesystem_diff(
                    &root,
                    &head_ref,
                    &path_filters,
                    &language_filters,
                    &previous_hashes,
                )
            })
            .await?
        } else {
            run_blocking_code(move || {
                changed_paths_for_diff_with_filters(
                    root,
                    &base_ref,
                    &head_ref,
                    &path_filters,
                    &language_filters,
                )
            })
            .await?
        };
        let registration = registration_from_status(&status);
        let path_groups = {
            let registration = registration.clone();
            let selector = request.repository.clone();
            let changed_paths = changed_paths.clone();
            run_blocking_code(move || {
                partition_changed_paths_for_selector(&registration, &selector, changed_paths)
            })
            .await?
        };
        let selector = request.repository.clone();
        let base_ref = request.base_ref.clone();
        let head_ref = head_commit;
        let deleted_symbol_names = run_blocking_code(move || {
            deleted_symbol_names_for_diff(&registration, &selector, &base_ref, &head_ref)
        })
        .await?;
        let source_scope = indexed_source_scope(&scoped_status)
            .ok_or_else(|| missing_indexed_source_scope_error(&scoped_status))?;
        let results = store
            .analyze_code_impact_scope(
                source_scope,
                request.clone(),
                CodeImpactChanges {
                    paths: changed_paths.clone(),
                    deleted_symbol_names,
                },
            )
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            request.head_ref.clone(),
        );

        Ok(CodeRepositoryImpactResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope,
            request,
            path_groups,
            results,
        })
    }

    pub async fn code_repository_status(
        &self,
        selector: CodeRepositorySelector,
        context: RequestContext,
    ) -> Result<CodeRepositoryStatusResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &selector.repository).await?;
        recover_code_index_task_leases(&store, now_millis()).await?;
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

        Ok(CodeRepositoryStatusResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
            active_task,
            checkpoint,
            retention,
        })
    }

    pub(crate) async fn code_repository_is_registered(
        &self,
        repository: String,
    ) -> Result<bool, ApiError> {
        let selector = CodeRepositorySelector::new(repository, "HEAD", Vec::new(), Vec::new())
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let store = self.store().await.map_err(storage_api_error)?;
        store
            .code_repository_status(selector.repository)
            .await
            .map(|status| status.is_some())
            .map_err(storage_api_error)
    }

    /// Builds a reusable operations report for a registered code repository.
    pub async fn code_repository_report(
        &self,
        selector: CodeRepositorySelector,
        context: RequestContext,
    ) -> Result<CodeRepositoryReportResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &selector.repository).await?;
        let report = store
            .code_repository_report(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositoryReportResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                &status,
                &selector,
                selector.ref_selector.clone(),
            ),
            report,
        })
    }
}

pub(super) fn annotate_query_result_staleness(
    results: &mut [CodeRetrievalHit],
    freshness: &crate::api::CodeRepositoryFreshnessDiagnostics,
) {
    let hint = if freshness.pending.active_matches_request && freshness.direct_source_read_required
    {
        StalenessHint::PendingIndex {}
    } else if freshness.direct_source_read_required {
        StalenessHint::Stale {}
    } else {
        StalenessHint::Fresh
    };
    let requires_source_verification = hint.requires_source_verification();
    for hit in results {
        if requires_source_verification {
            hit.stale = true;
        }
        if hint.should_replace(hit.staleness_hint.as_ref()) {
            hit.staleness_hint = Some(hint.clone());
        }
    }
}
