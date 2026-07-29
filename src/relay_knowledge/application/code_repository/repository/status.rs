//! Owns registered repository lookup, status checkpoints, and status conversion.

use crate::{
    api::ApiError,
    domain::{
        CodeIndexCheckpoint, CodeIndexTaskRecord, CodeRepositoryRegistration, CodeRepositoryStatus,
    },
};

use super::super::errors::storage_api_error;

pub(in crate::application::code_repository) async fn required_code_repository(
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

pub(in crate::application::code_repository) async fn code_status_checkpoint(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    active_task: Option<&CodeIndexTaskRecord>,
) -> Result<Option<CodeIndexCheckpoint>, ApiError> {
    if let Some(task) = active_task {
        return store
            .code_index_checkpoint(task.source_scope.clone())
            .await
            .map_err(storage_api_error);
    }
    if status.state == "indexing"
        && let Some(checkpoint) = store
            .latest_code_index_checkpoint(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?
    {
        return Ok(Some(checkpoint));
    }
    if let Some(scope) = status.last_indexed_scope_id.clone()
        && let Some(checkpoint) = store
            .code_index_checkpoint(scope)
            .await
            .map_err(storage_api_error)?
    {
        return Ok(Some(checkpoint));
    }

    Ok(None)
}

pub(in crate::application::code_repository) fn registration_from_status(
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
