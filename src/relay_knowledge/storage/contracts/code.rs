//! Storage contracts for code repository indexes.

use crate::domain::{
    CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeFileFingerprint, CodeImpactRequest,
    CodeIndexBatch, CodeIndexCheckpoint, CodeIndexPublicationFence, CodeIndexSession,
    CodeIndexSnapshot, CodeIndexSummary, CodeIndexTaskQueueStatus, CodeIndexTaskRecord,
    CodeRepositoryCrossEdge, CodeRepositoryRegistration, CodeRepositoryRemovalSummary,
    CodeRepositoryReport, CodeRepositorySet, CodeRepositorySetMember,
    CodeRepositorySetRefreshSummary, CodeRepositorySetRefreshTaskRecord, CodeRepositorySetStatus,
    CodeRepositoryStatus, CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest,
    CodeScopeRetentionSummary, CodeSymbolGenerationCounts, CodebaseViewRequest,
    CodebaseViewSnapshot, IndexedRepositoryDocument, SoftwareGlobalProjection,
    SoftwareGlobalRequest,
};

use super::{StorageError, StorageFuture};

/// Default error text for stores that do not support code task lease recovery.
pub const CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE: &str =
    "code index task lease recovery is unavailable";

/// Default error text for stores that do not support code task lease renewal.
pub const CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE: &str =
    "code index task lease renewal is unavailable";

/// Stable coarse states in the durable code-index finalization plan.
pub const CODE_INDEX_FINALIZATION_COARSE_PHASE_COUNT: usize = 11;

/// Hard bound for missing index units, coarse phases, and terminal observation.
pub const CODE_INDEX_FINALIZATION_MAX_STEPS: usize = crate::domain::CODE_QUERY_INDEX_PLAN_UNIT_COUNT
    + CODE_INDEX_FINALIZATION_COARSE_PHASE_COUNT
    + 2;

/// Derives the hard finalization quantum bound including worst-case
/// byte-limited reference resolution plus reference-search cleanup, group
/// discovery, and build pages.
pub fn code_index_finalization_max_steps(
    committed_reference_count: usize,
) -> Result<usize, StorageError> {
    committed_reference_count
        .checked_mul(4)
        .and_then(|pages| pages.checked_add(CODE_INDEX_FINALIZATION_MAX_STEPS + 6))
        .ok_or_else(|| {
            StorageError::CapacityExceeded(
                "reference-resolution and search finalization step bound exceeds platform capacity"
                    .to_owned(),
            )
        })
}

/// Result of advancing one durable code-index finalization writer quantum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeIndexFinalizationStep {
    Pending { checkpoint_state: String },
    Ready(Box<CodeIndexSummary>),
}

/// Diff-derived inputs used to seed code impact expansion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeImpactChanges {
    pub paths: Vec<String>,
    pub deleted_symbol_names: Vec<String>,
}

/// Bounded repository-set overlay keys needed to decorate retrieval candidates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeRepositorySetEdgeSelector {
    pub origin_files: Vec<(String, String)>,
    pub target_records: Vec<(String, String, String)>,
}

/// New background code index task to persist or deduplicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskSeed {
    pub repository_id: String,
    pub alias: String,
    pub ref_selector: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub source_scope: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub mode: crate::domain::CodeIndexMode,
    pub input_fingerprint: String,
    pub resource_budget: crate::domain::CodeIndexResourceBudget,
    pub payload_json: String,
    pub now_ms: u64,
}

/// Lease acquisition request for one background code index task.
///
/// `now_ms` is a caller observation. Storage samples authoritative execution
/// time only after obtaining its writer lock and rejects future observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskClaimRequest {
    pub task_id: Option<String>,
    pub lease_owner: String,
    pub lease_duration_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Strict renewal request for one still-live fenced code-index attempt.
///
/// Expiry is irrevocable. `now_ms` is only the caller's observation; storage
/// samples authoritative time after acquiring its writer lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskLeaseRenewal {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub publication_generation: u64,
    pub lease_duration_ms: u64,
    /// Caller-observed time used only to reject future/rollback observations.
    /// Storage samples authoritative time after obtaining its writer lock.
    pub now_ms: u64,
}

/// Active code-index task lease used by service startup recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskLeaseRecord {
    pub task_id: String,
    pub lease_owner: String,
    pub lease_expires_at_ms: Option<u64>,
    pub attempt_count: u32,
    pub publication_generation: u64,
}

/// Durable task target whose already-complete publication may be adopted by
/// a later fenced attempt without rebuilding code or software facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexPublicationTarget {
    pub task_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
}

/// Recovery request carrying the exact running leases observed as orphaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskLeaseRecovery {
    pub leases: Vec<CodeIndexTaskLeaseRecord>,
    pub now_ms: u64,
    pub max_attempts: u32,
    pub error_kind: String,
    pub error_message: String,
}

/// Completion report guarded by task lease and attempt token.
///
/// Expiry is irrevocable. `now_ms` is only the caller's observation; storage
/// samples authoritative time after acquiring its writer lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskCompletion {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub publication_generation: u64,
    pub now_ms: u64,
}

/// Failure report for retry and dead-letter handling.
///
/// Expiry is irrevocable. `now_ms` is only the caller's observation; storage
/// samples authoritative time after acquiring its writer lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskFailure {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub publication_generation: u64,
    pub error_kind: String,
    pub error_message: String,
    pub retry_backoff_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Scope retention request after a repository index completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeScopeRetentionRequest {
    pub repository_id: String,
    pub active_scope: String,
    pub retain_recent_successful_scopes: usize,
    /// Whole-repository wall-clock cutoff used for legacy publications and checkpoints.
    pub repository_retention_cutoff_ms: Option<u64>,
    /// Publication generation current when whole-repository retention was scheduled.
    /// Newer generations remain protected even when timestamps share one millisecond.
    pub repository_retention_cutoff_generation: Option<u64>,
    /// Scope that was current when whole-repository retention was scheduled.
    pub repository_retention_initial_scope: Option<String>,
}

/// New repository set metadata to persist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetSeed {
    pub alias: String,
    pub description: Option<String>,
    pub default_ref_policy_json: String,
    pub now_ms: u64,
}

/// New or replaced repository-set member pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetMemberSeed {
    pub set_alias: String,
    pub repository_id: String,
    pub repository_alias: String,
    pub ref_selector: String,
    pub resolved_commit_sha: String,
    pub source_scope: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub priority: i32,
}

/// Repository-set overlay refresh task to persist or deduplicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshTaskSeed {
    pub set_id: String,
    pub set_alias: String,
    pub input_fingerprint: String,
    pub now_ms: u64,
}

/// Lease acquisition request for one repository-set overlay task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshTaskClaimRequest {
    pub task_id: Option<String>,
    pub lease_owner: String,
    pub lease_duration_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Attempt-scoped authority required to publish a repository-set overlay.
///
/// Storage implementations must validate every field against a live running
/// task in the same transaction that replaces the overlay rows. A worker that
/// loses its lease or is superseded by a later attempt therefore cannot publish
/// a stale DELETE/INSERT sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshPublication {
    pub task_id: String,
    pub set_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub member_replacements: Vec<CodeRepositorySetMemberSeed>,
}

/// Completion report guarded by task lease and attempt token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshTaskCompletion {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub now_ms: u64,
}

/// Failure report for retry and dead-letter handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositorySetRefreshTaskFailure {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub error_kind: String,
    pub error_message: String,
    pub retry_backoff_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Persisted code repository graph and retrieval contract.
pub trait CodeRepositoryStore: Send + Sync {
    fn upsert_code_repository(
        &self,
        registration: CodeRepositoryRegistration,
    ) -> StorageFuture<'_, CodeRepositoryStatus>;

    fn code_repository_status(
        &self,
        repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>>;

    fn list_code_repositories(&self) -> StorageFuture<'_, Vec<CodeRepositoryStatus>> {
        Box::pin(async { Ok(Vec::new()) })
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
        _repository: String,
        _path_filters: Vec<String>,
        _language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        Box::pin(async { Ok(None) })
    }

    fn queue_code_index_task(
        &self,
        task: CodeIndexTaskSeed,
    ) -> StorageFuture<'_, CodeIndexTaskRecord>;

    fn claim_code_index_task(
        &self,
        request: CodeIndexTaskClaimRequest,
    ) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

    fn recover_code_index_task_leases(
        &self,
        _now_ms: u64,
        _max_attempts: u32,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE.to_owned(),
            ))
        })
    }

    fn running_code_index_task_leases(&self) -> StorageFuture<'_, Vec<CodeIndexTaskLeaseRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn recover_code_index_task_leases_by_task(
        &self,
        _request: CodeIndexTaskLeaseRecovery,
    ) -> StorageFuture<'_, usize> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE.to_owned(),
            ))
        })
    }

    fn reset_code_index_tasks(
        &self,
        _repository_id: String,
        _now_ms: u64,
    ) -> StorageFuture<'_, Vec<CodeIndexTaskRecord>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code index task reset is unavailable".to_owned(),
            ))
        })
    }

    fn renew_code_index_task_lease(
        &self,
        _request: CodeIndexTaskLeaseRenewal,
    ) -> StorageFuture<'_, CodeIndexTaskRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE.to_owned(),
            ))
        })
    }

    fn complete_code_index_task(
        &self,
        request: CodeIndexTaskCompletion,
    ) -> StorageFuture<'_, CodeIndexTaskRecord>;

    fn run_code_index_post_maintenance(
        &self,
        _repository_id: String,
        _source_scope: String,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn code_index_publication_receipt(
        &self,
        _task_id: String,
        _repository_id: String,
        _source_scope: String,
        _now_ms: u64,
    ) -> StorageFuture<'_, bool> {
        Box::pin(async { Ok(false) })
    }

    fn reconcile_code_index_publication_with_fence(
        &self,
        _target: CodeIndexPublicationTarget,
        _fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, bool> {
        Box::pin(async { Ok(false) })
    }

    fn fail_code_index_task(
        &self,
        request: CodeIndexTaskFailure,
    ) -> StorageFuture<'_, CodeIndexTaskRecord>;

    fn code_index_task(&self, task_id: String) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

    fn active_code_index_task(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

    fn code_index_task_queue_status(&self) -> StorageFuture<'_, CodeIndexTaskQueueStatus> {
        Box::pin(async { Ok(CodeIndexTaskQueueStatus::default()) })
    }

    fn code_index_checkpoint(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>>;

    fn latest_code_index_checkpoint(
        &self,
        _repository_id: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
        Box::pin(async { Ok(None) })
    }

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
        Box::pin(async { Ok(None) })
    }

    fn code_repository_retention_scan_pending(&self) -> StorageFuture<'_, bool> {
        Box::pin(async { Ok(false) })
    }

    fn code_file_fingerprints(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>>;

    fn code_file_fingerprints_for_scope(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code file fingerprints for scope '{source_scope}' are unavailable"
            )))
        })
    }

    fn code_file_fingerprints_for_paths(
        &self,
        source_scope: String,
        paths: Vec<String>,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
        Box::pin(async move {
            let mut fingerprints = self.code_file_fingerprints_for_scope(source_scope).await?;
            fingerprints.retain(|fingerprint| paths.iter().any(|path| path == &fingerprint.path));
            Ok(fingerprints)
        })
    }

    fn code_file_candidate_paths_for_scope(
        &self,
        source_scope: String,
        _path_filters: Vec<String>,
        _language_filters: Vec<String>,
        _exclude_generated: bool,
        _limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "bounded code file candidate paths for scope '{source_scope}' are unavailable"
            )))
        })
    }

    fn code_file_candidate_paths_for_query_scope(
        &self,
        source_scope: String,
        _query: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        exclude_generated: bool,
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        self.code_file_candidate_paths_for_scope(
            source_scope,
            path_filters,
            language_filters,
            exclude_generated,
            limit,
        )
    }

    fn repository_documents_for_scope(
        &self,
        source_scope: String,
        _path_filters: Vec<String>,
        _max_files: usize,
        _max_bytes: usize,
    ) -> StorageFuture<'_, Vec<IndexedRepositoryDocument>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "repository documents for source scope '{source_scope}' are unavailable"
            )))
        })
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary>;

    fn apply_code_index_snapshot_with_fence(
        &self,
        snapshot: CodeIndexSnapshot,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped snapshot publication for task '{}' scope '{}' is unavailable",
                fence.task_id, snapshot.source_scope
            )))
        })
    }

    fn clear_code_workspace_state(
        &self,
        _repository_id: String,
        _source_scope: String,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    /// Reports whether repository-owned auto-detected workspace artifacts
    /// still exist and therefore require a durable disabled-mode cleanup.
    fn code_repository_auto_workspace_state_exists(
        &self,
        _repository_id: String,
    ) -> StorageFuture<'_, bool> {
        Box::pin(async { Ok(false) })
    }

    fn clear_code_workspace_state_with_fence(
        &self,
        repository_id: String,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped workspace publication for task '{}' repository '{}' scope '{}' is unavailable",
                fence.task_id, repository_id, source_scope
            )))
        })
    }

    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "checkpointed code index sessions for scope '{}' are unavailable",
                session.source_scope
            )))
        })
    }

    fn begin_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped session startup for task '{}' scope '{}' is unavailable",
                fence.task_id, session.source_scope
            )))
        })
    }

    /// Starts a checkpointed session only if the durable checkpoint still
    /// exactly matches the value observed during read-only plan validation.
    /// `None` means that no checkpoint may exist at transaction time.
    fn begin_code_index_session_at_checkpoint(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            let expectation = expected_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.source_scope.as_str())
                .unwrap_or("missing");
            Err(StorageError::InvalidInput(format!(
                "checkpoint-CAS session startup for scope '{}' at expectation '{}' is unavailable",
                session.source_scope, expectation
            )))
        })
    }

    /// Fenced variant of [`CodeRepositoryStore::begin_code_index_session_at_checkpoint`].
    fn begin_code_index_session_at_checkpoint_with_fence(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            let expectation = expected_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.source_scope.as_str())
                .unwrap_or("missing");
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped checkpoint-CAS session startup for task '{}' scope '{}' at expectation '{}' is unavailable",
                fence.task_id, session.source_scope, expectation
            )))
        })
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "checkpointed code index batches for scope '{}' are unavailable",
                batch.source_scope
            )))
        })
    }

    fn apply_code_index_batch_with_fence(
        &self,
        batch: CodeIndexBatch,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped batch publication for task '{}' scope '{}' is unavailable",
                fence.task_id, batch.source_scope
            )))
        })
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "checkpointed code index finalization for scope '{}' is unavailable",
                session.source_scope
            )))
        })
    }

    fn finalize_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped session finalization for task '{}' scope '{}' is unavailable",
                fence.task_id, session.source_scope
            )))
        })
    }

    fn advance_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexFinalizationStep> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped single-step finalization for task '{}' scope '{}' is unavailable",
                fence.task_id, session.source_scope
            )))
        })
    }

    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>>;

    fn search_code_feature_flags(
        &self,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code feature flag search for repository '{}' is unavailable",
                request.repository.repository
            )))
        })
    }

    fn search_code_feature_flags_scope(
        &self,
        source_scope: String,
        _request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code feature flag search for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn search_code_scope(
        &self,
        source_scope: String,
        _request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code search for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn analyze_code_impact(
        &self,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>>;

    fn analyze_code_impact_scope(
        &self,
        source_scope: String,
        _request: CodeImpactRequest,
        _changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code impact analysis for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn codebase_view_snapshot(
        &self,
        source_scope: String,
        _request: CodebaseViewRequest,
        _row_limit: usize,
    ) -> StorageFuture<'_, CodebaseViewSnapshot> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "codebase view snapshot for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn code_repository_totals(&self) -> StorageFuture<'_, CodeRepositoryTotals> {
        Box::pin(async { Ok(CodeRepositoryTotals::default()) })
    }

    fn code_repository_report(
        &self,
        repository: String,
    ) -> StorageFuture<'_, CodeRepositoryReport> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code repository report for '{repository}' is unavailable"
            )))
        })
    }

    fn code_repository_scope_symbol_generation_counts(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, CodeSymbolGenerationCounts> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code symbol generation counts for source scope '{source_scope}' are unavailable"
            )))
        })
    }

    fn refresh_software_global_projection(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "software global projection for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn refresh_software_global_projection_with_fence(
        &self,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped software projection for task '{}' scope '{}' is unavailable",
                fence.task_id, source_scope
            )))
        })
    }

    fn software_global_projection(
        &self,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "software global projection for repository '{}' is unavailable",
                request.repository.repository
            )))
        })
    }

    fn software_global_projection_for_scope(
        &self,
        source_scope: String,
        _request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "software global projection for source scope '{source_scope}' is unavailable"
            )))
        })
    }

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
        _set_alias: String,
    ) -> StorageFuture<'_, Option<CodeRepositorySet>> {
        Box::pin(async { Ok(None) })
    }

    fn code_repository_set_status(
        &self,
        _set_alias: String,
    ) -> StorageFuture<'_, Option<CodeRepositorySetStatus>> {
        Box::pin(async { Ok(None) })
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
        _set_id: String,
    ) -> StorageFuture<'_, Vec<CodeRepositoryCrossEdge>> {
        Box::pin(async { Ok(Vec::new()) })
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
        Box::pin(async { Ok(None) })
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

#[cfg(test)]
#[path = "code_tests.rs"]
mod tests;
