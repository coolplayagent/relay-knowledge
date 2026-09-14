use crate::domain::CodeScopeRetentionSummary;

use super::super::{StorageError, StorageFuture};
use super::CodeScopeRetentionRequest;

/// Scope and whole-repository retention planning and execution capability.
pub trait CodeScopeRetentionStore: Send + Sync {
    fn code_scope_retention(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, CodeScopeRetentionSummary>;

    fn prune_code_repository_scopes(
        &self,
        request: CodeScopeRetentionRequest,
    ) -> StorageFuture<'_, CodeScopeRetentionSummary>;

    fn schedule_code_repository_retention(
        &self,
        _max_indexed_repositories: usize,
        _now_ms: u64,
    ) -> StorageFuture<'_, Option<String>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code repository retention scheduling is unavailable".to_owned(),
            ))
        })
    }

    fn code_repository_retention_scan_pending(&self) -> StorageFuture<'_, bool> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code repository retention scan status is unavailable".to_owned(),
            ))
        })
    }
}
