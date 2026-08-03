use std::path::Path;

use rusqlite::Connection;

use super::{scope_filters as code_query_scope, software};

mod batch;
mod feature_flags;
mod generated;
mod impact;
pub(super) mod lifecycle;
mod query;
mod routes;
mod schema;
mod search;
mod set;
mod snapshot;
mod symbols;
mod tasks;
mod views;
mod workspace;

#[cfg(test)]
#[path = "tests/mod_tests.rs"]
mod code_tests;

#[cfg(test)]
#[path = "tests/scope_status.rs"]
mod code_scope_status_tests;

#[cfg(test)]
#[path = "tests/incremental_search.rs"]
mod code_incremental_search_tests;

#[cfg(test)]
#[path = "tests/cross_language_calls.rs"]
mod code_cross_language_call_tests;

#[cfg(test)]
#[path = "query/accuracy/mod.rs"]
mod code_query_accuracy_tests;

#[cfg(test)]
#[path = "tests/metadata.rs"]
mod code_metadata_tests;

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeFileFingerprint, CodeImpactRequest,
        CodeIndexBatch, CodeIndexCheckpoint, CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary,
        CodeRepositoryRegistration, CodeRepositoryReport, CodeRepositoryStatus,
        CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest, CodeSymbolGenerationCounts,
        CodebaseViewRequest, CodebaseViewSnapshot, SoftwareGlobalProjection, SoftwareGlobalRequest,
    },
    storage::{CodeImpactChanges, CodeRepositoryStore, StorageError, StorageFuture},
};

use super::SqliteGraphStore;
use lifecycle::{cleanup, removal, report, status};
pub(super) use search::SearchDocumentInserter;

const MAX_SYMBOL_SIGNATURE_LOOKUP_IDS_PER_STATEMENT: usize = 500;

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    schema::initialize_code_schema(connection)?;
    software::initialize_schema(connection)
}

pub(super) fn import_repository_from_database(
    connection: &mut Connection,
    source_path: &Path,
    repository_id: &str,
    source_scope: Option<&str>,
) -> Result<(), StorageError> {
    snapshot::import_repository_from_database(connection, source_path, repository_id, source_scope)
}

pub(super) fn repository_totals_excluding(
    connection: &mut Connection,
    excluded_repository_ids: &[String],
) -> Result<CodeRepositoryTotals, StorageError> {
    report::repository_totals_excluding(connection, excluded_repository_ids)
}

pub(super) fn prune_scopes_with_retained(
    connection: &mut Connection,
    request: crate::storage::CodeScopeRetentionRequest,
    extra_retained_scopes: Vec<String>,
) -> Result<crate::domain::CodeScopeRetentionSummary, StorageError> {
    tasks::prune_scopes_with_retained(connection, request, extra_retained_scopes)
}

impl CodeRepositoryStore for SqliteGraphStore {
    fn upsert_code_repository(
        &self,
        registration: CodeRepositoryRegistration,
    ) -> StorageFuture<'_, CodeRepositoryStatus> {
        self.run(move |connection| status::upsert_repository(connection, registration))
    }

    fn code_repository_status(
        &self,
        repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        self.run_read(move |connection| status::repository_status(connection, &repository))
    }

    fn list_code_repositories(&self) -> StorageFuture<'_, Vec<CodeRepositoryStatus>> {
        self.run_read(status::repository_statuses)
    }

    fn remove_code_repository(
        &self,
        repository: String,
        now_ms: u64,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositoryRemovalSummary>> {
        self.run(move |connection| removal::remove_repository(connection, &repository, now_ms))
    }

    fn code_repository_scope_status(
        &self,
        repository: String,
        resolved_commit_sha: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        self.run_read(move |connection| {
            status::repository_scope_status(
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
            status::latest_repository_scope_status(
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
        self.run(move |connection| tasks::queue_task(connection, task))
    }

    fn claim_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run(move |connection| tasks::claim_task(connection, request))
    }

    fn recover_code_index_task_leases(
        &self,
        now_ms: u64,
        max_attempts: u32,
    ) -> StorageFuture<'_, ()> {
        self.run(move |connection| {
            tasks::recover_expired_task_leases(connection, now_ms, max_attempts)
        })
    }

    fn running_code_index_task_leases(
        &self,
    ) -> StorageFuture<'_, Vec<crate::storage::CodeIndexTaskLeaseRecord>> {
        self.run_read(tasks::running_task_leases)
    }

    fn recover_code_index_task_leases_by_task(
        &self,
        request: crate::storage::CodeIndexTaskLeaseRecovery,
    ) -> StorageFuture<'_, usize> {
        self.run(move |connection| tasks::recover_task_leases_by_task(connection, request))
    }

    fn reset_code_index_tasks(
        &self,
        repository_id: String,
        now_ms: u64,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeIndexTaskRecord>> {
        self.run(move |connection| tasks::reset_tasks(connection, &repository_id, now_ms))
    }

    fn renew_code_index_task_lease(
        &self,
        request: crate::storage::CodeIndexTaskLeaseRenewal,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| tasks::renew_task_lease(connection, request))
    }

    fn complete_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| tasks::complete_task(connection, request))
    }

    fn fail_code_index_task(
        &self,
        request: crate::storage::CodeIndexTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.run(move |connection| tasks::fail_task(connection, request))
    }

    fn code_index_task(
        &self,
        task_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run_read(move |connection| tasks::task_by_id(connection, &task_id))
    }

    fn active_code_index_task(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.run_read(move |connection| tasks::active_task(connection, &repository_id))
    }

    fn code_index_task_queue_status(
        &self,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskQueueStatus> {
        self.run_read(tasks::queue_status)
    }

    fn code_index_checkpoint(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexCheckpoint>> {
        self.run_read(move |connection| tasks::checkpoint(connection, &source_scope))
    }

    fn latest_code_index_checkpoint(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexCheckpoint>> {
        self.run_read(move |connection| {
            tasks::latest_checkpoint_for_repository(connection, &repository_id)
        })
    }

    fn code_scope_retention(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        self.run_read(move |connection| tasks::retention_status(connection, &repository_id))
    }

    fn prune_code_repository_scopes(
        &self,
        request: crate::storage::CodeScopeRetentionRequest,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        self.run(move |connection| tasks::prune_scopes(connection, request))
    }

    fn code_file_fingerprints(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run_read(move |connection| snapshot::file_fingerprints(connection, &repository_id))
    }

    fn code_file_fingerprints_for_scope(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run_read(move |connection| {
            snapshot::file_fingerprints_for_scope(connection, &source_scope)
        })
    }

    fn code_file_fingerprints_for_paths(
        &self,
        source_scope: String,
        paths: Vec<String>,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run_read(move |connection| {
            snapshot::file_fingerprints_for_paths(connection, &source_scope, &paths)
        })
    }

    fn code_file_candidate_paths_for_scope(
        &self,
        source_scope: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        exclude_generated: bool,
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        self.run_read(move |connection| {
            snapshot::file_candidate_paths_for_scope(
                connection,
                &source_scope,
                &path_filters,
                &language_filters,
                exclude_generated,
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
        exclude_generated: bool,
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        self.run_read(move |connection| {
            snapshot::file_candidate_paths_for_query_scope(
                connection,
                &source_scope,
                &query,
                &path_filters,
                &language_filters,
                exclude_generated,
                limit,
            )
        })
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        let maintenance = self.maintenance.clone();
        self.run(move |connection| {
            let summary = snapshot::apply_snapshot(connection, snapshot)?;
            super::connection_runtime::maintenance::run_post_index_maintenance(
                connection,
                &maintenance,
            );

            Ok(summary)
        })
    }

    fn clear_code_workspace_state(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> StorageFuture<'_, ()> {
        self.run(move |connection| {
            workspace::clear_auto_workspace_state(connection, &repository_id, &source_scope)
        })
    }

    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| batch::begin_session(connection, session))
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| batch::apply_batch(connection, batch))
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        let maintenance = self.maintenance.clone();
        self.run(move |connection| {
            let summary = batch::finalize_session(connection, session)?;
            super::connection_runtime::maintenance::run_post_index_maintenance(
                connection,
                &maintenance,
            );

            Ok(summary)
        })
    }

    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read(move |connection| query::search_code(connection, request))
    }

    fn search_code_feature_flags(
        &self,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        self.run_read(move |connection| feature_flags::search(connection, request))
    }

    fn search_code_feature_flags_scope(
        &self,
        source_scope: String,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        self.run_read(move |connection| {
            feature_flags::search_scope(connection, &source_scope, request)
        })
    }

    fn search_code_scope(
        &self,
        source_scope: String,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read(move |connection| {
            query::search_code_scope(connection, &source_scope, request)
        })
    }

    fn analyze_code_impact(
        &self,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read(move |connection| impact::analyze_impact(connection, request, changes))
    }

    fn analyze_code_impact_scope(
        &self,
        source_scope: String,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read(move |connection| {
            impact::analyze_impact_scope(connection, &source_scope, request, changes)
        })
    }

    fn codebase_view_snapshot(
        &self,
        source_scope: String,
        request: CodebaseViewRequest,
        row_limit: usize,
    ) -> StorageFuture<'_, CodebaseViewSnapshot> {
        self.run_read(move |connection| {
            views::snapshot(connection, &source_scope, &request, row_limit)
        })
    }

    fn code_repository_totals(&self) -> StorageFuture<'_, CodeRepositoryTotals> {
        self.run_read(report::repository_totals)
    }

    fn code_repository_report(
        &self,
        repository: String,
    ) -> StorageFuture<'_, CodeRepositoryReport> {
        self.run_read(move |connection| report::repository_report(connection, &repository))
    }

    fn code_repository_scope_symbol_generation_counts(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, CodeSymbolGenerationCounts> {
        self.run_read(move |connection| {
            let counts = report::scope_symbol_generation_counts(connection, &source_scope)?;
            Ok(CodeSymbolGenerationCounts {
                handwritten_symbol_count: counts.handwritten,
                generated_symbol_count: counts.generated,
            })
        })
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
        self.run(move |connection| set::create_set(connection, seed))
    }

    fn add_code_repository_set_member(
        &self,
        seed: crate::storage::CodeRepositorySetMemberSeed,
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
        self.run_read(move |connection| set::set_status(connection, &set_alias))
    }

    fn refresh_code_repository_set_overlay(
        &self,
        set_alias: String,
        now_ms: u64,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshSummary> {
        self.run(move |connection| set::refresh_overlay(connection, &set_alias, now_ms))
    }

    fn code_repository_set_cross_edges(
        &self,
        set_id: String,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeRepositoryCrossEdge>> {
        self.run_read(move |connection| set::cross_edges_for_set(connection, &set_id))
    }

    fn code_repository_set_cross_edges_for_selector(
        &self,
        set_id: String,
        selector: crate::storage::CodeRepositorySetEdgeSelector,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeRepositoryCrossEdge>> {
        self.run_read(move |connection| {
            set::cross_edges_for_selector(connection, &set_id, &selector)
        })
    }

    fn queue_code_repository_set_refresh_task(
        &self,
        task: crate::storage::CodeRepositorySetRefreshTaskSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| set::refresh_tasks::queue_refresh_task(connection, task))
    }

    fn claim_code_repository_set_refresh_task(
        &self,
        request: crate::storage::CodeRepositorySetRefreshTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySetRefreshTaskRecord>> {
        self.run(move |connection| set::refresh_tasks::claim_refresh_task(connection, request))
    }

    fn complete_code_repository_set_refresh_task(
        &self,
        request: crate::storage::CodeRepositorySetRefreshTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| set::refresh_tasks::complete_refresh_task(connection, request))
    }

    fn fail_code_repository_set_refresh_task(
        &self,
        request: crate::storage::CodeRepositorySetRefreshTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.run(move |connection| set::refresh_tasks::fail_refresh_task(connection, request))
    }
}
