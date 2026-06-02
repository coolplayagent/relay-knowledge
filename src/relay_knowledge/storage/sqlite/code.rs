use rusqlite::Connection;

#[path = "code_query.rs"]
mod code_query;

#[path = "code_query_prepare.rs"]
mod code_query_prepare;

#[path = "code_query_hits.rs"]
mod code_query_hits;

#[path = "code_feature_flags.rs"]
mod code_feature_flags;

#[path = "code_query_scope.rs"]
mod code_query_scope;

#[path = "code_impact.rs"]
mod code_impact;

#[path = "code_report.rs"]
pub(super) mod code_report;

#[path = "code_schema.rs"]
mod code_schema;

#[path = "code_status.rs"]
mod code_status;

#[path = "code_batch.rs"]
mod code_batch;

#[path = "code_cleanup.rs"]
mod code_cleanup;

#[path = "code_tasks.rs"]
mod code_tasks;

#[path = "code_search.rs"]
mod code_search;

#[path = "code_snapshot.rs"]
mod code_snapshot;

#[path = "code_set.rs"]
mod code_set;

#[path = "code_set_tasks.rs"]
mod code_set_tasks;

#[path = "software.rs"]
mod software;

#[cfg(test)]
#[path = "code_tests.rs"]
mod code_tests;

#[cfg(test)]
#[path = "code_snapshot_candidate_paths_tests.rs"]
mod code_snapshot_candidate_paths_tests;

#[cfg(test)]
#[path = "code_incremental_search_tests.rs"]
mod code_incremental_search_tests;

#[cfg(test)]
#[path = "code_batch_finalize_tests.rs"]
mod code_batch_finalize_tests;

#[cfg(test)]
#[path = "code_batch_finalize_typescript_tests.rs"]
mod code_batch_finalize_typescript_tests;

#[cfg(test)]
#[path = "code_cross_language_call_tests.rs"]
mod code_cross_language_call_tests;

#[cfg(test)]
#[path = "code_batch_search_tests.rs"]
mod code_batch_search_tests;

#[cfg(test)]
#[path = "code_snapshot_progress_tests.rs"]
mod code_snapshot_progress_tests;

#[cfg(test)]
#[path = "code_query_accuracy_tests.rs"]
mod code_query_accuracy_tests;

#[cfg(test)]
#[path = "code_query_import_target_tests.rs"]
mod code_query_import_target_tests;

#[cfg(test)]
#[path = "code_query_import_ranking_tests.rs"]
mod code_query_import_ranking_tests;

#[cfg(test)]
#[path = "code_query_sbom_tests.rs"]
mod code_query_sbom_tests;

#[cfg(test)]
#[path = "code_query_line_context_tests.rs"]
mod code_query_line_context_tests;

#[cfg(test)]
#[path = "code_metadata_tests.rs"]
mod code_metadata_tests;

#[cfg(test)]
#[path = "code_tasks_tests.rs"]
mod code_tasks_tests;

#[cfg(test)]
#[path = "code_set_tasks_tests.rs"]
mod code_set_tasks_tests;

#[cfg(test)]
#[path = "code_set_tests.rs"]
mod code_set_tests;

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeFileFingerprint, CodeImpactRequest,
        CodeIndexBatch, CodeIndexCheckpoint, CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary,
        CodeRepositoryRegistration, CodeRepositoryReport, CodeRepositoryStatus,
        CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest, SoftwareGlobalProjection,
        SoftwareGlobalRequest,
    },
    storage::{CodeImpactChanges, CodeRepositoryStore, StorageError, StorageFuture},
};

use super::SqliteGraphStore;
pub(super) use code_search::SearchDocumentInserter;

const MAX_SYMBOL_SIGNATURE_LOOKUP_IDS_PER_STATEMENT: usize = 500;

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    code_schema::initialize_code_schema(connection)?;
    software::initialize_schema(connection)
}

impl CodeRepositoryStore for SqliteGraphStore {
    fn upsert_code_repository(
        &self,
        registration: CodeRepositoryRegistration,
    ) -> StorageFuture<'_, CodeRepositoryStatus> {
        self.run(move |connection| code_status::upsert_repository(connection, registration))
    }

    fn code_repository_status(
        &self,
        repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        self.run_read(move |connection| code_status::repository_status(connection, &repository))
    }

    fn code_repository_scope_status(
        &self,
        repository: String,
        resolved_commit_sha: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        self.run_read(move |connection| {
            code_status::repository_scope_status(
                connection,
                &repository,
                &resolved_commit_sha,
                &path_filters,
                &language_filters,
            )
        })
    }

    fn latest_code_repository_scope_status(
        &self,
        repository: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        self.run_read(move |connection| {
            code_status::latest_repository_scope_status(
                connection,
                &repository,
                &path_filters,
                &language_filters,
            )
        })
    }

    fn queue_code_index_task(
        &self,
        task: crate::storage::CodeIndexTaskSeed,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| code_tasks::queue_task(connection, task))
    }

    fn claim_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run(move |connection| code_tasks::claim_task(connection, request))
    }

    fn recover_code_index_task_leases(
        &self,
        now_ms: u64,
        max_attempts: u32,
    ) -> StorageFuture<'_, ()> {
        self.run(move |connection| {
            code_tasks::recover_expired_task_leases(connection, now_ms, max_attempts)
        })
    }

    fn renew_code_index_task_lease(
        &self,
        request: crate::storage::CodeIndexTaskLeaseRenewal,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| code_tasks::renew_task_lease(connection, request))
    }

    fn complete_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| code_tasks::complete_task(connection, request))
    }

    fn fail_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| code_tasks::fail_task(connection, request))
    }

    fn code_index_task(
        &self,
        task_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run_read(move |connection| code_tasks::task_by_id(connection, &task_id))
    }

    fn active_code_index_task(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run_read(move |connection| code_tasks::active_task(connection, &repository_id))
    }

    fn code_index_checkpoint(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexCheckpoint>> {
        self.run_read(move |connection| code_tasks::checkpoint(connection, &source_scope))
    }

    fn latest_code_index_checkpoint(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexCheckpoint>> {
        self.run_read(move |connection| {
            code_tasks::latest_checkpoint_for_repository(connection, &repository_id)
        })
    }

    fn code_scope_retention(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        self.run_read(move |connection| code_tasks::retention_status(connection, &repository_id))
    }

    fn prune_code_repository_scopes(
        &self,
        request: crate::storage::CodeScopeRetentionRequest,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        self.run(move |connection| code_tasks::prune_scopes(connection, request))
    }

    fn code_file_fingerprints(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run_read(move |connection| {
            code_snapshot::file_fingerprints(connection, &repository_id)
        })
    }

    fn code_file_fingerprints_for_scope(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run_read(move |connection| {
            code_snapshot::file_fingerprints_for_scope(connection, &source_scope)
        })
    }

    fn code_file_candidate_paths_for_scope(
        &self,
        source_scope: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        self.run_read(move |connection| {
            code_snapshot::file_candidate_paths_for_scope(
                connection,
                &source_scope,
                &path_filters,
                &language_filters,
                limit,
            )
        })
    }

    fn code_file_candidate_paths_for_query_scope(
        &self,
        source_scope: String,
        query: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        self.run_read(move |connection| {
            code_snapshot::file_candidate_paths_for_query_scope(
                connection,
                &source_scope,
                &query,
                &path_filters,
                &language_filters,
                limit,
            )
        })
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        self.run(move |connection| code_snapshot::apply_snapshot(connection, snapshot))
    }

    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| code_batch::begin_session(connection, session))
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| code_batch::apply_batch(connection, batch))
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        self.run(move |connection| code_batch::finalize_session(connection, session))
    }

    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read(move |connection| code_query::search_code(connection, request))
    }

    fn search_code_feature_flags(
        &self,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        self.run_read(move |connection| code_feature_flags::search(connection, request))
    }

    fn search_code_feature_flags_scope(
        &self,
        source_scope: String,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        self.run_read(move |connection| {
            code_feature_flags::search_scope(connection, &source_scope, request)
        })
    }

    fn search_code_scope(
        &self,
        source_scope: String,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read(move |connection| {
            code_query::search_code_scope(connection, &source_scope, request)
        })
    }

    fn analyze_code_impact(
        &self,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read(move |connection| code_impact::analyze_impact(connection, request, changes))
    }

    fn code_repository_totals(&self) -> StorageFuture<'_, CodeRepositoryTotals> {
        self.run_read(code_report::repository_totals)
    }

    fn code_repository_report(
        &self,
        repository: String,
    ) -> StorageFuture<'_, CodeRepositoryReport> {
        self.run_read(move |connection| code_report::repository_report(connection, &repository))
    }

    fn refresh_software_global_projection(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        self.run(move |connection| software::refresh_projection(connection, &source_scope))
    }

    fn software_global_projection(
        &self,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        self.run_read(move |connection| software::projection(connection, request))
    }

    fn software_global_projection_for_scope(
        &self,
        source_scope: String,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        self.run_read(move |connection| {
            software::projection_for_scope(connection, &source_scope, request)
        })
    }

    fn create_code_repository_set(
        &self,
        seed: crate::storage::CodeRepositorySetSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySet> {
        self.run(move |connection| code_set::create_set(connection, seed))
    }

    fn add_code_repository_set_member(
        &self,
        seed: crate::storage::CodeRepositorySetMemberSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetMember> {
        self.run(move |connection| code_set::add_member(connection, seed))
    }

    fn remove_code_repository_set_member(
        &self,
        set_alias: String,
        repository_alias: String,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetMember> {
        self.run(move |connection| {
            code_set::remove_member(connection, &set_alias, &repository_alias)
        })
    }

    fn code_repository_set(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySet>> {
        self.run_read(move |connection| code_set::set_by_alias(connection, &set_alias))
    }

    fn code_repository_set_status(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySetStatus>> {
        self.run_read(move |connection| code_set::set_status(connection, &set_alias))
    }

    fn refresh_code_repository_set_overlay(
        &self,
        set_alias: String,
        now_ms: u64,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshSummary> {
        self.run(move |connection| code_set::refresh_overlay(connection, &set_alias, now_ms))
    }

    fn code_repository_set_cross_edges(
        &self,
        set_id: String,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeRepositoryCrossEdge>> {
        self.run_read(move |connection| code_set::cross_edges_for_set(connection, &set_id))
    }

    fn queue_code_repository_set_refresh_task(
        &self,
        task: crate::storage::CodeRepositorySetRefreshTaskSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| code_set_tasks::queue_refresh_task(connection, task))
    }

    fn claim_code_repository_set_refresh_task(
        &self,
        request: crate::storage::CodeRepositorySetRefreshTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySetRefreshTaskRecord>> {
        self.run(move |connection| code_set_tasks::claim_refresh_task(connection, request))
    }

    fn complete_code_repository_set_refresh_task(
        &self,
        request: crate::storage::CodeRepositorySetRefreshTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| code_set_tasks::complete_refresh_task(connection, request))
    }

    fn fail_code_repository_set_refresh_task(
        &self,
        request: crate::storage::CodeRepositorySetRefreshTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| code_set_tasks::fail_refresh_task(connection, request))
    }
}
