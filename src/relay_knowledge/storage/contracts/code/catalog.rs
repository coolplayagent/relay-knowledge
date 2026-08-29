use crate::domain::{
    CodeRepositoryRegistration, CodeRepositoryRemovalSummary, CodeRepositoryStatus,
};

use super::super::{StorageError, StorageFuture};

/// Repository registration, lookup, and lifecycle capability.
pub trait RepositoryCatalogStore: Send + Sync {
    fn upsert_code_repository(
        &self,
        registration: CodeRepositoryRegistration,
    ) -> StorageFuture<'_, CodeRepositoryStatus>;

    fn code_repository_status(
        &self,
        repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>>;

    fn list_code_repositories(&self) -> StorageFuture<'_, Vec<CodeRepositoryStatus>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code repository catalog listing is unavailable".to_owned(),
            ))
        })
    }

    fn remove_code_repository(
        &self,
        repository: String,
        now_ms: u64,
    ) -> StorageFuture<'_, Option<CodeRepositoryRemovalSummary>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code repository removal for '{repository}' at {now_ms} is unavailable"
            )))
        })
    }

    fn code_repository_scope_status(
        &self,
        repository: String,
        resolved_commit_sha: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>>;

    fn latest_code_repository_scope_status(
        &self,
        repository: String,
        _path_filters: Vec<String>,
        _language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "latest code repository scope for '{repository}' is unavailable"
            )))
        })
    }
}
