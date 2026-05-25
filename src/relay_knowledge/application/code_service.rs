use std::path::PathBuf;

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryFeatureFlagsResponse, CodeRepositoryImpactResponse,
        CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse, CodeRepositoryQueryResponse,
        CodeRepositoryRegisterRequest, CodeRepositoryRegisterResponse,
        CodeRepositoryReportResponse, CodeRepositoryScopePreviewResponse,
        CodeRepositoryStatusResponse, RequestContext,
    },
    code::{
        CodeIndexError, SOURCE_GREP_CANDIDATE_FILE_LIMIT, build_index_snapshot,
        changed_paths_for_diff, deleted_symbol_names_for_diff,
        partition_changed_paths_for_selector, prepare_full_index_plan, preview_repository_scope,
        register_repository, resolve_repository_ref, resolve_repository_snapshot,
        source_declarations_for_identity, source_grep_matches,
    },
    domain::{
        CodeFeatureFlagRequest, CodeImpactRequest, CodeIndexMode, CodeIndexRequest,
        CodeIndexResourceBudget, CodeRepositoryRegistration, CodeRepositorySelector,
        CodeRepositoryStatus, CodeRetrievalRequest, FreshnessPolicy,
        code_snapshot_expected_scope_id as expected_scope_id,
    },
    storage::{CodeImpactChanges, StorageError},
};

use super::RelayKnowledgeService;
use super::code_query_source_fallback::{
    append_code_grep_fallback, append_definition_source_fallback, plan_code_grep_fallback,
};

const CODE_INDEX_TASK_LEASE_MS: u64 = 30 * 60 * 1000;
const CODE_INDEX_TASK_MAX_ATTEMPTS: u32 = 3;
const CODE_INDEX_TASK_RETRY_BACKOFF_MS: u64 = 60_000;
const RETAIN_RECENT_CODE_SCOPES: usize = 2;

impl RelayKnowledgeService {
    /// Registers a Git repository as a code source.
    pub async fn register_code_repository(
        &self,
        request: CodeRepositoryRegisterRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryRegisterResponse, ApiError> {
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

    /// Builds or updates the tree-sitter code index for a registered repository.
    pub async fn index_code_repository(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if let Some(response) = self
            .fresh_full_index_response(&store, &status, &request, &context)
            .await?
        {
            return Ok(response);
        }
        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let summary = if request.mode == CodeIndexMode::Full {
            let resource_budget = CodeIndexResourceBudget::default();
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
                let (next_plan, batch) = run_blocking_code(move || plan.parse_next_batch()).await?;
                plan = next_plan;
                let Some(batch) = batch else {
                    break;
                };
                store
                    .apply_code_index_batch(batch)
                    .await
                    .map_err(storage_api_error)?;
            }
            store
                .finalize_code_index_session(session)
                .await
                .map_err(storage_api_error)?
        } else {
            let previous = previous_fingerprints_for_index(&store, &status, &request).await?;
            let mode = request.mode;
            let snapshot = run_blocking_code(move || {
                build_index_snapshot(&registration, &selector, mode, previous)
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
            status,
        })
    }

    /// Starts a repository index request, queueing cold full indexes for background execution.
    pub async fn start_code_repository_index(
        &self,
        request: CodeIndexRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryIndexStartResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        if let Some(response) = self
            .fresh_full_index_response(&store, &status, &request, &context)
            .await?
        {
            return Ok(index_start_from_completed(response, None));
        }
        if request.mode != CodeIndexMode::Full {
            let response = self.index_code_repository(request, context).await?;
            return Ok(index_start_from_completed(response, None));
        }

        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let resource_budget = CodeIndexResourceBudget::default();
        let plan = run_blocking_code(move || {
            prepare_full_index_plan(registration, selector, resource_budget)
        })
        .await?;
        let session = plan.session();
        let payload_json = serde_json::to_string(&request)
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let input_fingerprint = format!(
            "full:{}:{}:{}:{}",
            session.repository_id,
            session.resolved_commit_sha,
            session.tree_hash,
            session.source_scope
        );
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
            .unwrap_or(status);

        Ok(CodeRepositoryIndexStartResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_index_task(
                &task,
                request.repository.ref_selector,
            ),
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
        let lease_owner = format!("code-index-worker-{}", std::process::id());
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
        let result = self.index_code_repository(request, context).await;
        match result {
            Ok(response) => {
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

    async fn fresh_full_index_response(
        &self,
        store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
        status: &CodeRepositoryStatus,
        request: &CodeIndexRequest,
        context: &RequestContext,
    ) -> Result<Option<CodeRepositoryIndexResponse>, ApiError> {
        if request.mode != CodeIndexMode::Full {
            return Ok(None);
        }
        let registration = registration_from_status(status);
        let selector = request.repository.clone();
        let (resolved_commit_sha, tree_hash) = run_blocking_code(move || {
            resolve_repository_snapshot(&registration.root_path, &selector.ref_selector)
        })
        .await?;
        let path_filters = merged_filters(&status.path_filters, &request.repository.path_filters);
        let language_filters = merged_filters(
            &status.language_filters,
            &request.repository.language_filters,
        );
        let expected_source_scope = expected_scope_id(
            &status.repository_id,
            &tree_hash,
            &path_filters,
            &language_filters,
        );
        let scoped_status = store
            .code_repository_scope_status(
                request.repository.repository.clone(),
                resolved_commit_sha.clone(),
                path_filters,
                language_filters,
            )
            .await
            .map_err(storage_api_error)?;
        let Some(scoped_status) = scoped_status else {
            return Ok(None);
        };
        if scoped_status.stale
            || scoped_status.tree_hash.as_deref() != Some(tree_hash.as_str())
            || expected_source_scope.as_deref().is_some_and(|expected| {
                scoped_status.last_indexed_scope_id.as_deref() != Some(expected)
            })
        {
            return Ok(None);
        }
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let report = store
            .code_repository_report(scoped_status.repository_id.clone())
            .await
            .map_err(storage_api_error)?;
        let summary = crate::domain::CodeIndexSummary {
            repository_id: scoped_status.repository_id.clone(),
            source_scope: scoped_status
                .last_indexed_scope_id
                .clone()
                .unwrap_or_default(),
            resolved_commit_sha,
            tree_hash,
            indexed_file_count: scoped_status.indexed_file_count,
            changed_path_count: 0,
            skipped_unchanged_count: scoped_status.indexed_file_count,
            deleted_path_count: 0,
            symbol_count: scoped_status.symbol_count,
            reference_count: scoped_status.reference_count,
            chunk_count: scoped_status.chunk_count,
            degraded_file_count: report.degraded_file_count,
            progress: crate::domain::CodeIndexProgressSummary {
                git_file_count: scoped_status.indexed_file_count,
                blob_read_count: 0,
                parsed_file_count: 0,
                sqlite_write_count: 0,
                skipped_file_count: scoped_status.indexed_file_count,
                degraded_file_count: report.degraded_file_count,
                batch_count: 0,
                checkpoint_file_count: scoped_status.indexed_file_count,
                resource_budget: crate::domain::CodeIndexResourceBudget::default(),
            },
        };

        Ok(Some(CodeRepositoryIndexResponse {
            metadata: ApiMetadata::graph_only(context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                &scoped_status,
                &request.repository,
                request.repository.ref_selector.clone(),
            ),
            summary,
            status: scoped_status,
        }))
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
            return Ok(CodeRepositoryQueryResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                    &status,
                    &request.repository,
                    request.repository.ref_selector.clone(),
                ),
                request,
                results: Vec::new(),
                degraded_reason: Some("graph_only freshness policy selected".to_owned()),
            });
        }
        let requested_ref = request.repository.ref_selector.clone();
        let request = retrieval_request_at_indexed_ref(request, &status).await?;
        let scoped_status =
            resolved_code_scope_status(&store, &status, &request.repository).await?;
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
        let mut results = store
            .search_code(request.clone())
            .await
            .map_err(storage_api_error)?;
        let fallback_degraded_reason =
            apply_code_grep_fallback(&store, &status, &scoped_status, &request, &mut results)
                .await?;
        let degraded_reason = results
            .iter()
            .find_map(|hit| hit.degraded_reason.clone())
            .or(fallback_degraded_reason)
            .or_else(|| scoped_status.degraded_reason.clone());
        let scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            requested_ref,
        );

        Ok(CodeRepositoryQueryResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope,
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
            return Ok(CodeRepositoryFeatureFlagsResponse {
                metadata: ApiMetadata::graph_only(&context, graph_version),
                scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                    &status,
                    &request.repository,
                    request.repository.ref_selector.clone(),
                ),
                request,
                flags: Vec::new(),
                degraded_reason: Some("graph_only freshness policy selected".to_owned()),
            });
        }
        let requested_ref = request.repository.ref_selector.clone();
        let request = feature_flag_request_at_indexed_ref(request, &status).await?;
        let scoped_status =
            resolved_code_scope_status(&store, &status, &request.repository).await?;
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
        let flags = store
            .search_code_feature_flags(request.clone())
            .await
            .map_err(storage_api_error)?;
        let scope = crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            requested_ref,
        );
        let degraded_reason = scoped_status.degraded_reason.clone();

        Ok(CodeRepositoryFeatureFlagsResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope,
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
        let head_commit = resolve_code_ref(&status, request.head_ref.clone()).await?;
        request.repository.ref_selector = head_commit.clone();
        let scoped_status =
            resolved_code_scope_status(&store, &status, &request.repository).await?;
        let root = PathBuf::from(status.root_path.clone());
        let base_ref = request.base_ref.clone();
        let head_ref = head_commit.clone();
        let changed_paths =
            run_blocking_code(move || changed_paths_for_diff(root, &base_ref, &head_ref)).await?;
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
        let results = store
            .analyze_code_impact(
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
        let active_task = store
            .active_code_index_task(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?;
        let checkpoint = match active_task.as_ref() {
            Some(task) => store
                .code_index_checkpoint(task.source_scope.clone())
                .await
                .map_err(storage_api_error)?,
            None => match status.last_indexed_scope_id.clone() {
                Some(scope) => store
                    .code_index_checkpoint(scope)
                    .await
                    .map_err(storage_api_error)?,
                None => None,
            },
        };
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

async fn required_code_repository(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    repository: &str,
) -> Result<crate::domain::CodeRepositoryStatus, ApiError> {
    store
        .code_repository_status(repository.to_owned())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!("code repository '{repository}' is not registered"))
        })
}

fn registration_from_status(
    status: &crate::domain::CodeRepositoryStatus,
) -> CodeRepositoryRegistration {
    CodeRepositoryRegistration {
        repository_id: status.repository_id.clone(),
        alias: status.alias.clone(),
        root_path: status.root_path.clone(),
        path_filters: status.path_filters.clone(),
        language_filters: status.language_filters.clone(),
    }
}

fn index_start_from_completed(
    response: CodeRepositoryIndexResponse,
    task: Option<crate::domain::CodeIndexTaskRecord>,
) -> CodeRepositoryIndexStartResponse {
    CodeRepositoryIndexStartResponse {
        metadata: response.metadata,
        scope: response.scope,
        summary: Some(response.summary),
        status: response.status,
        task,
        checkpoint: None,
    }
}

async fn previous_fingerprints_for_index(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
) -> Result<Vec<crate::domain::CodeFileFingerprint>, ApiError> {
    let CodeIndexMode::Incremental { base_ref, .. } = &request.mode else {
        return store
            .code_file_fingerprints(status.repository_id.clone())
            .await
            .map_err(storage_api_error);
    };
    let base_commit = resolve_code_ref(status, base_ref.clone()).await?;
    let path_filters = merged_filters(&status.path_filters, &request.repository.path_filters);
    let language_filters = merged_filters(
        &status.language_filters,
        &request.repository.language_filters,
    );
    let base_scope = store
        .code_repository_scope_status(
            request.repository.repository.clone(),
            base_commit.clone(),
            path_filters,
            language_filters,
        )
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "incremental base ref '{}' resolves to {}, but code repository '{}' has no matching indexed base scope; run repo index --ref {} before repo update",
                base_ref, base_commit, status.alias, base_ref
            ))
        })?;
    if base_scope.stale {
        return Err(ApiError::invalid_argument(format!(
            "incremental base ref '{}' resolves to a stale indexed scope {}; refresh or reindex the base before repo update",
            base_ref,
            base_scope
                .last_indexed_scope_id
                .as_deref()
                .unwrap_or("unscoped")
        )));
    }
    let source_scope = base_scope.last_indexed_scope_id.ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "incremental base ref '{}' has no persisted source scope",
            base_ref
        ))
    })?;

    store
        .code_file_fingerprints_for_scope(source_scope)
        .await
        .map_err(storage_api_error)
}

pub(crate) async fn apply_code_grep_fallback(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    base_status: &CodeRepositoryStatus,
    scoped_status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &mut Vec<crate::domain::CodeRetrievalHit>,
) -> Result<Option<String>, ApiError> {
    let Some(plan) = plan_code_grep_fallback(scoped_status, request, results) else {
        return Ok(None);
    };
    let plan = if plan.needs_scope_paths() {
        let source_scope = scoped_status
            .last_indexed_scope_id
            .as_deref()
            .ok_or_else(|| {
                ApiError::invalid_argument(format!(
                    "code repository '{}' does not have an indexed source scope",
                    scoped_status.alias
                ))
            })?;
        let paths = match store
            .code_file_candidate_paths_for_scope(
                source_scope.to_owned(),
                plan.path_filters.clone(),
                plan.language_filters.clone(),
                SOURCE_GREP_CANDIDATE_FILE_LIMIT.saturating_add(1),
            )
            .await
        {
            Ok(paths) => paths,
            Err(error) => {
                return Ok(Some(format!(
                    "ripgrep candidate path lookup unavailable: {error}"
                )));
            }
        };
        plan.with_scope_paths(paths)
    } else {
        plan
    };
    let registration = registration_from_status(base_status);
    let commit = plan.commit.clone();
    let source_request = plan.source_request();
    let outcome =
        run_blocking_code(move || source_grep_matches(&registration, &commit, source_request))
            .await?;
    let had_matches = !outcome.matches.is_empty();
    let fallback_degraded_reason =
        append_code_grep_fallback(scoped_status, request, results, &plan, outcome);
    if !had_matches
        && plan.kind == crate::code::SourceGrepKind::Definition
        && let Some(identity) = &plan.identity
    {
        let registration = registration_from_status(base_status);
        let commit = plan.commit.clone();
        let paths = plan.paths.clone();
        let identity = identity.clone();
        let declarations = run_blocking_code(move || {
            source_declarations_for_identity(&registration, &commit, paths, &identity)
        })
        .await?;
        append_definition_source_fallback(scoped_status, request, results, declarations);
    }

    Ok(fallback_degraded_reason)
}

async fn retrieval_request_at_indexed_ref(
    mut request: CodeRetrievalRequest,
    status: &CodeRepositoryStatus,
) -> Result<CodeRetrievalRequest, ApiError> {
    request.repository.ref_selector =
        indexed_commit_for_ref(status, request.repository.ref_selector.clone()).await?;

    Ok(request)
}

async fn feature_flag_request_at_indexed_ref(
    mut request: CodeFeatureFlagRequest,
    status: &CodeRepositoryStatus,
) -> Result<CodeFeatureFlagRequest, ApiError> {
    request.repository.ref_selector =
        indexed_commit_for_ref(status, request.repository.ref_selector.clone()).await?;

    Ok(request)
}

async fn resolved_code_scope_status(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
) -> Result<CodeRepositoryStatus, ApiError> {
    let path_filters = merged_filters(&status.path_filters, &selector.path_filters);
    let language_filters = merged_filters(&status.language_filters, &selector.language_filters);
    let exact_scope = store
        .code_repository_scope_status(
            selector.repository.clone(),
            selector.ref_selector.clone(),
            path_filters,
            language_filters,
        )
        .await
        .map_err(storage_api_error)?;
    let scoped_status = match exact_scope {
        Some(status) => Some(status),
        None if (!selector.path_filters.is_empty() || !selector.language_filters.is_empty())
            && selector_filters_fit_indexed_scope(status, selector) =>
        {
            store
                .code_repository_scope_status(
                    selector.repository.clone(),
                    selector.ref_selector.clone(),
                    status.path_filters.clone(),
                    status.language_filters.clone(),
                )
                .await
                .map_err(storage_api_error)?
        }
        None => None,
    };
    scoped_status.ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "code repository '{}' has no index for ref {} and requested filters",
            selector.repository, selector.ref_selector
        ))
    })
}

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}

fn selector_filters_fit_indexed_scope(
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
) -> bool {
    requested_paths_fit_indexed_scope(&status.path_filters, &selector.path_filters)
        && requested_languages_fit_indexed_scope(
            &status.language_filters,
            &selector.language_filters,
        )
}

fn requested_paths_fit_indexed_scope(
    indexed_filters: &[String],
    selector_filters: &[String],
) -> bool {
    selector_filters.is_empty()
        || indexed_filters.is_empty()
        || selector_filters.iter().all(|selector_filter| {
            indexed_filters
                .iter()
                .any(|indexed_filter| path_filter_covers(indexed_filter, selector_filter))
        })
}

fn requested_languages_fit_indexed_scope(
    indexed_filters: &[String],
    selector_filters: &[String],
) -> bool {
    selector_filters.is_empty()
        || indexed_filters.is_empty()
        || selector_filters
            .iter()
            .all(|selector_filter| indexed_filters.contains(selector_filter))
}

fn path_filter_covers(indexed_filter: &str, selector_filter: &str) -> bool {
    let indexed_filter = normalize_path_filter(indexed_filter);
    let selector_filter = normalize_path_filter(selector_filter);
    indexed_filter == "."
        || (!indexed_filter.is_empty()
            && !selector_filter.is_empty()
            && (selector_filter == indexed_filter
                || selector_filter.starts_with(&format!("{indexed_filter}/"))))
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

async fn indexed_commit_for_ref(
    status: &CodeRepositoryStatus,
    ref_selector: String,
) -> Result<String, ApiError> {
    if ref_selector == "worktree" {
        if is_worktree_overlay(status) {
            return status.last_indexed_commit.clone().ok_or_else(|| {
                ApiError::invalid_argument(format!(
                    "code repository '{}' has no active worktree overlay",
                    status.alias
                ))
            });
        }
        return Err(ApiError::invalid_argument(format!(
            "code repository '{}' has no active worktree overlay",
            status.alias
        )));
    }

    resolve_code_ref(status, ref_selector).await
}

fn is_worktree_overlay(status: &CodeRepositoryStatus) -> bool {
    status
        .last_indexed_commit
        .as_deref()
        .is_some_and(|value| value.starts_with("worktree:"))
        || status
            .tree_hash
            .as_deref()
            .is_some_and(|value| value.starts_with("worktree:"))
}

async fn resolve_code_ref(
    status: &CodeRepositoryStatus,
    ref_selector: String,
) -> Result<String, ApiError> {
    let root = PathBuf::from(status.root_path.clone());

    run_blocking_code(move || resolve_repository_ref(root, &ref_selector)).await
}

async fn run_blocking_code<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CodeIndexError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
        .map_err(code_api_error)
}

fn code_api_error(error: CodeIndexError) -> ApiError {
    match error {
        CodeIndexError::InvalidInput(message) => ApiError::invalid_argument(message),
        CodeIndexError::Git { .. } | CodeIndexError::Io(_) | CodeIndexError::TreeSitter(_) => {
            ApiError::storage_unavailable(error.to_string())
        }
    }
}

fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}
