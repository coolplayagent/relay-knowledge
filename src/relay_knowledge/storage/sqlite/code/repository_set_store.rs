use crate::storage::{
    CodeRepositorySetEdgeSelector, CodeRepositorySetMemberSeed,
    CodeRepositorySetRefreshPublication, CodeRepositorySetRefreshTaskClaimRequest,
    CodeRepositorySetRefreshTaskCompletion, CodeRepositorySetRefreshTaskFailure,
    CodeRepositorySetRefreshTaskSeed, CodeRepositorySetSeed, CodeRepositorySetStore, StorageFuture,
};

use super::{SqliteGraphStore, set};

impl CodeRepositorySetStore for SqliteGraphStore {
    fn create_code_repository_set(
        &self,
        seed: CodeRepositorySetSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySet> {
        self.run(move |connection| set::create_set(connection, seed))
    }

    fn add_code_repository_set_member(
        &self,
        seed: CodeRepositorySetMemberSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetMember> {
        self.run(move |connection| set::add_member(connection, seed))
    }

    fn remove_code_repository_set_member(
        &self,
        set_alias: String,
        repository_alias: String,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetMember> {
        self.run(move |connection| set::remove_member(connection, &set_alias, &repository_alias))
    }

    fn code_repository_set(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySet>> {
        self.run_read(move |connection| set::set_by_alias(connection, &set_alias))
    }

    fn code_repository_set_status(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySetStatus>> {
        self.run_read_snapshot(move |connection| set::set_status(connection, &set_alias))
    }

    fn refresh_code_repository_set_overlay(
        &self,
        set_alias: String,
        publication: CodeRepositorySetRefreshPublication,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshSummary> {
        self.run(move |connection| {
            set::refresh_overlay_for_task(connection, &set_alias, publication)
        })
    }

    fn code_repository_set_cross_edges(
        &self,
        set_id: String,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeRepositoryCrossEdge>> {
        self.run_read_snapshot(move |connection| set::cross_edges_for_set(connection, &set_id))
    }

    fn code_repository_set_cross_edges_for_selector(
        &self,
        set_id: String,
        selector: CodeRepositorySetEdgeSelector,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeRepositoryCrossEdge>> {
        self.run_read_snapshot(move |connection| {
            set::cross_edges_for_selector(connection, &set_id, &selector)
        })
    }

    fn queue_code_repository_set_refresh_task(
        &self,
        task: CodeRepositorySetRefreshTaskSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| set::refresh_tasks::queue_refresh_task(connection, task))
    }

    fn claim_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySetRefreshTaskRecord>> {
        self.run(move |connection| set::refresh_tasks::claim_refresh_task(connection, request))
    }

    fn complete_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| set::refresh_tasks::complete_refresh_task(connection, request))
    }

    fn fail_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| set::refresh_tasks::fail_refresh_task(connection, request))
    }
}
