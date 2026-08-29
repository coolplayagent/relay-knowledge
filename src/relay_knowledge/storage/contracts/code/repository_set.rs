use crate::domain::{
    CodeRepositoryCrossEdge, CodeRepositorySet, CodeRepositorySetMember,
    CodeRepositorySetRefreshSummary, CodeRepositorySetRefreshTaskRecord, CodeRepositorySetStatus,
};

use super::super::{StorageError, StorageFuture};
use super::{
    CodeRepositorySetEdgeSelector, CodeRepositorySetMemberSeed,
    CodeRepositorySetRefreshPublication, CodeRepositorySetRefreshTaskClaimRequest,
    CodeRepositorySetRefreshTaskCompletion, CodeRepositorySetRefreshTaskFailure,
    CodeRepositorySetRefreshTaskSeed, CodeRepositorySetSeed,
};

/// Repository-set membership, overlay, and refresh-task capability.
pub trait CodeRepositorySetStore: Send + Sync {
    fn create_code_repository_set(
        &self,
        _seed: CodeRepositorySetSeed,
    ) -> StorageFuture<'_, CodeRepositorySet> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "repository set storage is unavailable".to_owned(),
            ))
        })
    }

    fn add_code_repository_set_member(
        &self,
        _seed: CodeRepositorySetMemberSeed,
    ) -> StorageFuture<'_, CodeRepositorySetMember> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "repository set member storage is unavailable".to_owned(),
            ))
        })
    }

    fn remove_code_repository_set_member(
        &self,
        _set_alias: String,
        _repository_alias: String,
    ) -> StorageFuture<'_, CodeRepositorySetMember> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "repository set member storage is unavailable".to_owned(),
            ))
        })
    }

    fn code_repository_set(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<CodeRepositorySet>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "repository set lookup for '{set_alias}' is unavailable"
            )))
        })
    }

    fn code_repository_set_status(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<CodeRepositorySetStatus>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "repository set status for '{set_alias}' is unavailable"
            )))
        })
    }

    fn refresh_code_repository_set_overlay(
        &self,
        _set_alias: String,
        _publication: CodeRepositorySetRefreshPublication,
    ) -> StorageFuture<'_, CodeRepositorySetRefreshSummary> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "repository set overlay refresh is unavailable".to_owned(),
            ))
        })
    }

    fn code_repository_set_cross_edges(
        &self,
        set_id: String,
    ) -> StorageFuture<'_, Vec<CodeRepositoryCrossEdge>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "repository set cross edges for '{set_id}' are unavailable"
            )))
        })
    }

    fn code_repository_set_cross_edges_for_selector(
        &self,
        set_id: String,
        _selector: CodeRepositorySetEdgeSelector,
    ) -> StorageFuture<'_, Vec<CodeRepositoryCrossEdge>> {
        self.code_repository_set_cross_edges(set_id)
    }

    fn queue_code_repository_set_refresh_task(
        &self,
        _task: CodeRepositorySetRefreshTaskSeed,
    ) -> StorageFuture<'_, CodeRepositorySetRefreshTaskRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "repository set refresh task storage is unavailable".to_owned(),
            ))
        })
    }

    fn claim_code_repository_set_refresh_task(
        &self,
        _request: CodeRepositorySetRefreshTaskClaimRequest,
    ) -> StorageFuture<'_, Option<CodeRepositorySetRefreshTaskRecord>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "repository set refresh task claim is unavailable".to_owned(),
            ))
        })
    }

    fn complete_code_repository_set_refresh_task(
        &self,
        _request: CodeRepositorySetRefreshTaskCompletion,
    ) -> StorageFuture<'_, CodeRepositorySetRefreshTaskRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "repository set refresh task storage is unavailable".to_owned(),
            ))
        })
    }

    fn fail_code_repository_set_refresh_task(
        &self,
        _request: CodeRepositorySetRefreshTaskFailure,
    ) -> StorageFuture<'_, CodeRepositorySetRefreshTaskRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "repository set refresh task storage is unavailable".to_owned(),
            ))
        })
    }
}
