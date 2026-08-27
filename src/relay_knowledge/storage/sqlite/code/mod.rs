use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use super::{business, scope_filters as code_query_scope, software};

mod batch;
mod checkpoint_receipt;
mod documents;
mod feature_flags;
mod frameworks;
mod generated;
mod impact;
pub(in crate::storage) mod lifecycle;
pub(in crate::storage::sqlite) mod publication;
mod query;
mod routes;
pub(in crate::storage::sqlite) mod schema;
mod search;
mod session_finalization;
mod set;
mod snapshot;
mod symbols;
mod tasks;
mod views;
mod workspace;

#[cfg(test)]
pub(in crate::storage) use schema::ensure_code_query_indexes;

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

#[cfg(test)]
#[path = "tests/unfenced_authority.rs"]
mod code_unfenced_authority_tests;

use crate::{
    domain::{
        BusinessKnowledgeProjection, BusinessKnowledgeProjectionInput,
        BusinessKnowledgeQueryRequest, BusinessKnowledgeStatus, CodeFeatureFlagGraph,
        CodeFeatureFlagRequest, CodeFileFingerprint, CodeImpactRequest, CodeIndexBatch,
        CodeIndexCheckpoint, CodeIndexPublicationFence, CodeIndexSession, CodeIndexSnapshot,
        CodeIndexSummary, CodeRepositoryRegistration, CodeRepositoryReport, CodeRepositoryStatus,
        CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest, CodeSymbolGenerationCounts,
        CodebaseViewRequest, CodebaseViewSnapshot, IndexedRepositoryDocument,
        SoftwareGlobalProjection, SoftwareGlobalRequest,
    },
    storage::{
        BusinessKnowledgeStore, CodeImpactChanges, CodeRepositoryStore, StorageError, StorageFuture,
    },
};

use super::SqliteGraphStore;
pub(in crate::storage) use lifecycle::commit_scope::{
    preserve_existing_scope_commit, record as record_commit_scope,
};
use lifecycle::{cleanup, removal, report, status};
pub(in crate::storage) use publication::record_receipt_from_active_fence;
pub(super) use search::SearchDocumentInserter;
#[cfg(test)]
pub(in crate::storage) use tasks::MAX_SCOPE_SLOTS_PER_REPOSITORY;

impl SqliteGraphStore {
    pub(in crate::storage) fn code_query_indexes_ready_for_publication(
        &self,
    ) -> StorageFuture<'_, bool> {
        self.run_read(|connection| schema::query_indexes_ready_for_fact_publication(connection))
    }

    pub(in crate::storage) fn materialize_partitioned_completed_checkpoint(
        &self,
        expected: CodeIndexCheckpoint,
        fence: Option<CodeIndexPublicationFence>,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = fence
                .map(|fence| {
                    lifecycle::publication_fence::prepare_guard(
                        connection,
                        fence,
                        authority_path.as_deref(),
                    )
                })
                .transpose()?;
            batch::materialize_partitioned_completed_checkpoint(
                connection,
                expected,
                guard.as_ref(),
            )
        })
    }

    pub(in crate::storage) fn reopen_completed_checkpoint_for_partitioned_repair(
        &self,
        expected: CodeIndexCheckpoint,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            batch::reopen_completed_checkpoint_for_partitioned_repair(connection, expected, &guard)
        })
    }
}

pub(super) fn initialize_code_schema(connection: &Connection) -> Result<(), StorageError> {
    schema::initialize_code_schema(connection)?;
    software::initialize_schema(connection)?;
    business::initialize_schema(connection)
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

pub(super) fn complete_repository_retention(
    connection: &mut Connection,
    repository_id: &str,
    cutoff_ms: u64,
) -> Result<bool, StorageError> {
    tasks::complete_repository_retention(connection, repository_id, cutoff_ms)
}

pub(super) fn repository_retention_republished_initial_scope(
    connection: &Connection,
    repository_id: &str,
    initial_scope: &str,
    cutoff_ms: u64,
    cutoff_publication_generation: u64,
) -> Result<Option<String>, StorageError> {
    tasks::repository_retention_republished_initial_scope(
        connection,
        repository_id,
        initial_scope,
        cutoff_ms,
        cutoff_publication_generation,
    )
}

fn ensure_queryable_code_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    tasks::retention_gc::reject_retiring_scope(connection, source_scope)?;
    let stale = connection
        .query_row(
            "SELECT stale FROM code_repository_scopes WHERE source_scope = ?1",
            [source_scope],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "code repository scope '{source_scope}' is unavailable"
            ))
        })?;
    if stale {
        return Err(StorageError::InvalidInput(format!(
            "code repository scope '{source_scope}' is not published"
        )));
    }
    #[cfg(test)]
    read_snapshot_test_hook::after_retiring_check();
    Ok(())
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
        self.run_read_snapshot(move |connection| status::repository_status(connection, &repository))
    }

    fn list_code_repositories(&self) -> StorageFuture<'_, Vec<CodeRepositoryStatus>> {
        self.run_read_snapshot(status::repository_statuses)
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
        self.run_read_snapshot(move |connection| {
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
        self.run_read_snapshot(move |connection| {
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

    fn run_code_index_post_maintenance(
        &self,
        _repository_id: String,
        _source_scope: String,
    ) -> StorageFuture<'_, ()> {
        let maintenance = self.maintenance.clone();
        self.run(move |connection| {
            super::connection_runtime::maintenance::run_post_index_maintenance(
                connection,
                &maintenance,
            );
            Ok(())
        })
    }

    fn code_index_publication_receipt(
        &self,
        task_id: String,
        repository_id: String,
        source_scope: String,
        now_ms: u64,
    ) -> StorageFuture<'_, bool> {
        self.run_read(move |connection| {
            tasks::publication_receipt(connection, &task_id, &repository_id, &source_scope, now_ms)
        })
    }

    fn reconcile_code_index_publication_with_fence(
        &self,
        target: crate::storage::CodeIndexPublicationTarget,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, bool> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            publication::adopt_active_target(connection, &target, &guard)
        })
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
        self.run_read_snapshot(move |connection| {
            tasks::retention_status(connection, &repository_id)
        })
    }

    fn prune_code_repository_scopes(
        &self,
        request: crate::storage::CodeScopeRetentionRequest,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        self.run(move |connection| tasks::prune_scopes(connection, request))
    }

    fn schedule_code_repository_retention(
        &self,
        max_indexed_repositories: usize,
        now_ms: u64,
    ) -> StorageFuture<'_, Option<String>> {
        self.run(move |connection| {
            tasks::schedule_repository_retention(connection, max_indexed_repositories, now_ms)
        })
    }

    fn code_repository_retention_scan_pending(&self) -> StorageFuture<'_, bool> {
        self.run_read(|connection| tasks::repository_retention_scan_pending(connection))
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
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            snapshot::file_fingerprints_for_scope(connection, &source_scope)
        })
    }

    fn code_file_fingerprints_for_paths(
        &self,
        source_scope: String,
        paths: Vec<String>,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
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
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
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
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
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

    fn repository_documents_for_scope(
        &self,
        source_scope: String,
        path_filters: Vec<String>,
        max_files: usize,
        max_bytes: usize,
    ) -> StorageFuture<'_, Vec<IndexedRepositoryDocument>> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            documents::read_indexed_markdown_in_snapshot(
                connection,
                &source_scope,
                &path_filters,
                max_files,
                max_bytes,
            )
        })
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        let this = self.clone();
        Box::pin(async move {
            let summary = this
                .run(move |connection| snapshot::apply_snapshot(connection, snapshot))
                .await?;
            session_finalization::run_best_effort_maintenance(&this).await;
            Ok(summary)
        })
    }

    fn apply_code_index_snapshot_with_fence(
        &self,
        snapshot: CodeIndexSnapshot,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            snapshot::apply_snapshot_with_fence(connection, snapshot, Some(&guard))
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

    fn code_repository_auto_workspace_state_exists(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, bool> {
        self.run_read(move |connection| {
            workspace::has_auto_workspace_state(connection, &repository_id)
        })
    }

    fn clear_code_workspace_state_with_fence(
        &self,
        repository_id: String,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, ()> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            workspace::clear_auto_workspace_state_with_fence(
                connection,
                &repository_id,
                &source_scope,
                &guard,
            )
        })
    }

    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| batch::begin_session(connection, session))
    }

    fn begin_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            batch::begin_session_with_fence(connection, session, Some(&guard))
        })
    }

    fn begin_code_index_session_at_checkpoint(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| {
            batch::begin_session_at_checkpoint(connection, session, expected_checkpoint)
        })
    }

    fn begin_code_index_session_at_checkpoint_with_fence(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            batch::begin_session_at_checkpoint_with_fence(
                connection,
                session,
                expected_checkpoint,
                Some(&guard),
            )
        })
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        self.run(move |connection| batch::apply_batch(connection, batch))
    }

    fn apply_code_index_batch_with_fence(
        &self,
        batch: CodeIndexBatch,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            batch::apply_batch_with_fence(connection, batch, Some(&guard))
        })
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        session_finalization::finalize_session(self, session)
    }

    fn finalize_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        session_finalization::finalize_session_with_fence(self, session, fence)
    }

    fn advance_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, crate::storage::CodeIndexFinalizationStep> {
        session_finalization::advance_session_with_fence(self, session, fence)
    }

    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read_snapshot(move |connection| query::search_code(connection, request))
    }

    fn search_code_feature_flags(
        &self,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        self.run_read_snapshot(move |connection| feature_flags::search(connection, request))
    }

    fn search_code_feature_flags_scope(
        &self,
        source_scope: String,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            feature_flags::search_scope(connection, &source_scope, request)
        })
    }

    fn search_code_scope(
        &self,
        source_scope: String,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            query::search_code_scope(connection, &source_scope, request)
        })
    }

    fn analyze_code_impact(
        &self,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read_snapshot(move |connection| {
            impact::analyze_impact(connection, request, changes)
        })
    }

    fn analyze_code_impact_scope(
        &self,
        source_scope: String,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            impact::analyze_impact_scope(connection, &source_scope, request, changes)
        })
    }

    fn codebase_view_snapshot(
        &self,
        source_scope: String,
        request: CodebaseViewRequest,
        row_limit: usize,
    ) -> StorageFuture<'_, CodebaseViewSnapshot> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
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
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
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
        self.run(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            software::refresh_projection(connection, &source_scope)
        })
    }

    fn refresh_software_global_projection_with_fence(
        &self,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            software::refresh_projection_with_fence(connection, &source_scope, Some(&guard))
        })
    }

    fn software_global_projection(
        &self,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        self.run_read_snapshot(move |connection| software::projection(connection, request))
    }

    fn software_global_projection_for_scope(
        &self,
        source_scope: String,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
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
        self.run_read_snapshot(move |connection| set::set_status(connection, &set_alias))
    }

    fn refresh_code_repository_set_overlay(
        &self,
        set_alias: String,
        publication: crate::storage::CodeRepositorySetRefreshPublication,
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
        selector: crate::storage::CodeRepositorySetEdgeSelector,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeRepositoryCrossEdge>> {
        self.run_read_snapshot(move |connection| {
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

impl BusinessKnowledgeStore for SqliteGraphStore {
    fn replace_business_knowledge_projection(
        &self,
        input: BusinessKnowledgeProjectionInput,
    ) -> StorageFuture<'_, BusinessKnowledgeStatus> {
        self.run(move |connection| business::replace_projection(connection, input, None))
    }

    fn replace_business_knowledge_projection_with_fence(
        &self,
        input: BusinessKnowledgeProjectionInput,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, BusinessKnowledgeStatus> {
        let authority_path = self.publication_authority_path.clone();
        self.run(move |connection| {
            let guard = lifecycle::publication_fence::prepare_guard(
                connection,
                fence,
                authority_path.as_deref(),
            )?;
            business::replace_projection(connection, input, Some(&guard))
        })
    }

    fn business_knowledge_projection_for_scope(
        &self,
        source_scope: String,
        request: BusinessKnowledgeQueryRequest,
    ) -> StorageFuture<'_, BusinessKnowledgeProjection> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            business::projection_for_scope(connection, &source_scope, request)
        })
    }

    fn business_knowledge_status(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<BusinessKnowledgeStatus>> {
        self.run_read_snapshot(move |connection| {
            business::status_for_scope(connection, &source_scope)
        })
    }
}

#[cfg(test)]
#[path = "tests/read_snapshot_test_hook.rs"]
mod read_snapshot_test_hook;
