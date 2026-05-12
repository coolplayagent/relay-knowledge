use std::path::PathBuf;

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryImpactResponse, CodeRepositoryIndexResponse,
        CodeRepositoryQueryResponse, CodeRepositoryRegisterRequest, CodeRepositoryRegisterResponse,
        CodeRepositoryStatusResponse, RequestContext,
    },
    code::{CodeIndexError, build_index_snapshot, changed_paths_for_diff, register_repository},
    domain::{
        CodeImpactRequest, CodeIndexRequest, CodeRepositoryRegistration, CodeRepositorySelector,
        CodeRetrievalRequest,
    },
    storage::StorageError,
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
            request,
            results,
            degraded_reason,
        })
    }

    /// Returns impact radius for a Git diff using the indexed code graph.
    pub async fn impact_code_repository(
        &self,
        request: CodeImpactRequest,
        context: RequestContext,
    ) -> Result<CodeRepositoryImpactResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(&store, &request.repository.repository).await?;
        let root = PathBuf::from(status.root_path);
        let base_ref = request.base_ref.clone();
        let head_ref = request.head_ref.clone();
        let changed_paths =
            run_blocking_code(move || changed_paths_for_diff(root, &base_ref, &head_ref)).await?;
        let results = store
            .analyze_code_impact(request.clone(), changed_paths.clone())
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositoryImpactResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
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
