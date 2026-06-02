//! Storage contracts for code repository indexes.

use crate::domain::{
    CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeFileFingerprint, CodeImpactRequest,
    CodeIndexBatch, CodeIndexCheckpoint, CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary,
    CodeIndexTaskRecord, CodeRepositoryCrossEdge, CodeRepositoryRegistration, CodeRepositoryReport,
    CodeRepositorySet, CodeRepositorySetMember, CodeRepositorySetRefreshSummary,
    CodeRepositorySetRefreshTaskRecord, CodeRepositorySetStatus, CodeRepositoryStatus,
    CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest, CodeScopeRetentionSummary,
    SoftwareGlobalProjection, SoftwareGlobalRequest,
};

use super::{StorageError, StorageFuture};

/// Default error text for stores that do not support code task lease recovery.
pub const CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE: &str =
    "code index task lease recovery is unavailable";

/// Default error text for stores that do not support code task lease renewal.
pub const CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE: &str =
    "code index task lease renewal is unavailable";

/// Diff-derived inputs used to seed code impact expansion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeImpactChanges {
    pub paths: Vec<String>,
    pub deleted_symbol_names: Vec<String>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskClaimRequest {
    pub task_id: Option<String>,
    pub lease_owner: String,
    pub lease_duration_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Lease renewal request for an actively running code index task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskLeaseRenewal {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub lease_duration_ms: u64,
    pub now_ms: u64,
}

/// Completion report guarded by task lease and attempt token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskCompletion {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub now_ms: u64,
}

/// Failure report for retry and dead-letter handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeIndexTaskFailure {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
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

    fn fail_code_index_task(
        &self,
        request: CodeIndexTaskFailure,
    ) -> StorageFuture<'_, CodeIndexTaskRecord>;

    fn code_index_task(&self, task_id: String) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

    fn active_code_index_task(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

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

    fn code_file_candidate_paths_for_scope(
        &self,
        source_scope: String,
        _path_filters: Vec<String>,
        _language_filters: Vec<String>,
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
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        self.code_file_candidate_paths_for_scope(
            source_scope,
            path_filters,
            language_filters,
            limit,
        )
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary>;

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
        _now_ms: u64,
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
