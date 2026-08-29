#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
mod staleness;
mod status;
#[cfg(test)]
#[path = "test_support.rs"]
pub(super) mod test_support;
#[cfg(test)]
#[path = "workspace_scope_tests.rs"]
mod workspace_scope_tests;
mod worktree;
#[cfg(test)]
#[path = "worktree_review_tests.rs"]
mod worktree_review_tests;

use crate::{
    api::{
        ApiError, ApiMetadata, CodeRepositoryListResponse, CodeRepositoryRegisterRequest,
        CodeRepositoryRegisterResponse, CodeRepositoryRemoveResponse, CodeRepositoryReportResponse,
        CodeRepositoryStatusResponse, RequestContext,
    },
    code::{REGISTRATION_LANGUAGE_FILTER_ERROR, register_repository},
    domain::CodeRepositorySelector,
};
use std::path::PathBuf;

use crate::application::service::RelayKnowledgeService;

use super::{
    blocking::run_blocking_code, clock::now_millis, errors::storage_api_error,
    indexing::recover_code_index_task_leases,
};

pub(super) use staleness::annotate_query_result_staleness;
pub(super) use status::{
    code_status_checkpoint, registration_from_status, required_code_repository,
};
pub(super) use worktree::ensure_worktree_overlay_matches_current_worktree;

impl RelayKnowledgeService {
    /// Resolves a repository root only through persisted managed-repository identity.
    pub(crate) async fn registered_code_repository_root(
        &self,
        repository: &str,
    ) -> Result<PathBuf, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(store.as_ref(), repository).await?;

        Ok(PathBuf::from(status.root_path))
    }

    /// Lists repositories that have at least one completed indexed scope.
    pub async fn list_indexed_code_repositories(
        &self,
        context: RequestContext,
    ) -> Result<CodeRepositoryListResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let repositories = store
            .list_code_repositories()
            .await
            .map_err(storage_api_error)?
            .into_iter()
            .filter(|status| status.last_indexed_scope_id.is_some())
            .collect();
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositoryListResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            repositories,
        })
    }

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
        let _ = self.refresh_watched_code_repository(&status).await;
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
        let removed_status = required_code_repository(store.as_ref(), &repository).await?;
        let summary = store
            .remove_code_repository(removed_status.repository_id.clone(), now_ms)
            .await
            .map_err(storage_api_error)?
            .ok_or_else(|| {
                ApiError::storage_unavailable("removed code repository disappeared before delete")
            })?;
        let _ = self
            .remove_watched_code_repository(&removed_status.alias, &removed_status.repository_id)
            .await;
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

    pub async fn code_repository_status(
        &self,
        selector: CodeRepositorySelector,
        context: RequestContext,
    ) -> Result<CodeRepositoryStatusResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_code_repository(store.as_ref(), &selector.repository).await?;
        recover_code_index_task_leases(&store, now_millis()).await?;
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
        let status = required_code_repository(store.as_ref(), &selector.repository).await?;
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
