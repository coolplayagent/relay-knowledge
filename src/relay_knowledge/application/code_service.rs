use std::path::PathBuf;

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryImpactResponse, CodeRepositoryIndexResponse,
        CodeRepositoryQueryResponse, CodeRepositoryRegisterRequest, CodeRepositoryRegisterResponse,
        CodeRepositoryStatusResponse, RequestContext,
    },
    code::{
        CodeIndexError, build_index_snapshot, changed_paths_for_diff,
        deleted_symbol_names_for_diff, register_repository, resolve_repository_ref,
    },
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalRequest, FreshnessPolicy,
    },
    storage::{CodeImpactChanges, StorageError},
};

use super::RelayKnowledgeService;

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
        validate_index_mode_against_status(&status, &request.mode).await?;
        let previous = store
            .code_file_fingerprints(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?;
        let registration = registration_from_status(&status);
        let selector = request.repository.clone();
        let mode = request.mode;
        let snapshot = run_blocking_code(move || {
            build_index_snapshot(&registration, &selector, mode, previous)
        })
        .await?;
        let summary = store
            .apply_code_index_snapshot(snapshot)
            .await
            .map_err(storage_api_error)?;
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
        if request.freshness_policy == FreshnessPolicy::WaitUntilFresh && status.stale {
            return Err(ApiError::invalid_argument(format!(
                "code repository '{}' is stale; run repo index or repo update before querying with wait_until_fresh",
                status.alias
            )));
        }
        let requested_ref = request.repository.ref_selector.clone();
        let request = retrieval_request_at_indexed_ref(request, &status).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;
        let results = store
            .search_code(request.clone())
            .await
            .map_err(storage_api_error)?;
        let degraded_reason = results.iter().find_map(|hit| hit.degraded_reason.clone());

        Ok(CodeRepositoryQueryResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                &status,
                &request.repository,
                requested_ref,
            ),
            request,
            results,
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
        let indexed_commit =
            indexed_commit_for_ref(&status, request.repository.ref_selector.clone()).await?;
        let head_commit = resolve_code_ref(&status, request.head_ref.clone()).await?;
        if head_commit != indexed_commit {
            return Err(ApiError::invalid_argument(format!(
                "impact head ref '{}' resolves to {}, but code repository '{}' is indexed at {}",
                request.head_ref, head_commit, status.alias, indexed_commit
            )));
        }
        request.repository.ref_selector = indexed_commit;
        let root = PathBuf::from(status.root_path.clone());
        let base_ref = request.base_ref.clone();
        let head_ref = head_commit.clone();
        let changed_paths =
            run_blocking_code(move || changed_paths_for_diff(root, &base_ref, &head_ref)).await?;
        let registration = registration_from_status(&status);
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

        Ok(CodeRepositoryImpactResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            scope: crate::api::CodeRepositoryScopeMetadata::from_status(
                &status,
                &request.repository,
                request.head_ref.clone(),
            ),
            request,
            changed_paths,
            results,
        })
    }

    /// Returns the current code repository index status.
    pub async fn code_repository_status(
        &self,
        selector: CodeRepositorySelector,
        context: RequestContext,
    ) -> Result<CodeRepositoryStatusResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &selector.repository).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositoryStatusResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
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

async fn validate_index_mode_against_status(
    status: &CodeRepositoryStatus,
    mode: &CodeIndexMode,
) -> Result<(), ApiError> {
    let CodeIndexMode::Incremental { base_ref, .. } = mode else {
        return Ok(());
    };
    let indexed_commit = status.last_indexed_commit.as_deref().ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "code repository '{}' must be fully indexed before incremental indexing",
            status.alias
        ))
    })?;
    let base_commit = resolve_code_ref(status, base_ref.clone()).await?;
    if base_commit != indexed_commit {
        return Err(ApiError::invalid_argument(format!(
            "incremental base ref '{}' resolves to {}, but code repository '{}' is indexed at {}",
            base_ref, base_commit, status.alias, indexed_commit
        )));
    }

    Ok(())
}

async fn retrieval_request_at_indexed_ref(
    mut request: CodeRetrievalRequest,
    status: &CodeRepositoryStatus,
) -> Result<CodeRetrievalRequest, ApiError> {
    request.repository.ref_selector =
        indexed_commit_for_ref(status, request.repository.ref_selector.clone()).await?;

    Ok(request)
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

    let requested_commit = resolve_code_ref(status, ref_selector).await?;
    if status.last_indexed_commit.as_deref() != Some(requested_commit.as_str()) {
        return Err(ApiError::invalid_argument(format!(
            "code repository '{}' is indexed at {}, not requested ref {}",
            status.alias,
            status.last_indexed_commit.as_deref().unwrap_or("nothing"),
            requested_commit
        )));
    }

    Ok(requested_commit)
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
