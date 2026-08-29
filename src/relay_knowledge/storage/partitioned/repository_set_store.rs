use crate::{
    domain::{
        CodeRepositoryCrossEdge, CodeRepositorySet, CodeRepositorySetMember,
        CodeRepositorySetRefreshSummary, CodeRepositorySetStatus,
    },
    storage::{
        CodeRepositorySetEdgeSelector, CodeRepositorySetMemberSeed,
        CodeRepositorySetRefreshPublication, CodeRepositorySetRefreshTaskClaimRequest,
        CodeRepositorySetRefreshTaskCompletion, CodeRepositorySetRefreshTaskFailure,
        CodeRepositorySetRefreshTaskSeed, CodeRepositorySetSeed, CodeRepositorySetStore,
        StorageError, StorageFuture,
    },
};

use super::PartitionedSqliteKnowledgeStore;

impl CodeRepositorySetStore for PartitionedSqliteKnowledgeStore {
    fn create_code_repository_set(
        &self,
        seed: CodeRepositorySetSeed,
    ) -> StorageFuture<'_, CodeRepositorySet> {
        self.control.create_code_repository_set(seed)
    }

    fn add_code_repository_set_member(
        &self,
        seed: CodeRepositorySetMemberSeed,
    ) -> StorageFuture<'_, CodeRepositorySetMember> {
        self.control.add_code_repository_set_member(seed)
    }

    fn remove_code_repository_set_member(
        &self,
        set_alias: String,
        repository_alias: String,
    ) -> StorageFuture<'_, CodeRepositorySetMember> {
        self.control
            .remove_code_repository_set_member(set_alias, repository_alias)
    }

    fn code_repository_set(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<CodeRepositorySet>> {
        self.control.code_repository_set(set_alias)
    }

    fn code_repository_set_status(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<CodeRepositorySetStatus>> {
        self.control.code_repository_set_status(set_alias)
    }

    fn refresh_code_repository_set_overlay(
        &self,
        set_alias: String,
        _publication: CodeRepositorySetRefreshPublication,
    ) -> StorageFuture<'_, CodeRepositorySetRefreshSummary> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "repository set overlay refresh for '{set_alias}' requires the single_sqlite topology until cross-shard import/export aggregation is implemented"
            )))
        })
    }

    fn code_repository_set_cross_edges(
        &self,
        set_id: String,
    ) -> StorageFuture<'_, Vec<CodeRepositoryCrossEdge>> {
        self.control.code_repository_set_cross_edges(set_id)
    }

    fn code_repository_set_cross_edges_for_selector(
        &self,
        set_id: String,
        selector: CodeRepositorySetEdgeSelector,
    ) -> StorageFuture<'_, Vec<CodeRepositoryCrossEdge>> {
        self.control
            .code_repository_set_cross_edges_for_selector(set_id, selector)
    }

    fn queue_code_repository_set_refresh_task(
        &self,
        task: CodeRepositorySetRefreshTaskSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "repository set overlay refresh task for '{}' requires the single_sqlite topology until cross-shard import/export aggregation is implemented",
                task.set_alias
            )))
        })
    }

    fn claim_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySetRefreshTaskRecord>> {
        self.control.claim_code_repository_set_refresh_task(request)
    }

    fn complete_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.control
            .complete_code_repository_set_refresh_task(request)
    }

    fn fail_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.control.fail_code_repository_set_refresh_task(request)
    }
}
