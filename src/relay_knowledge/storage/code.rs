//! Storage contracts for code repository indexes.

use crate::domain::{
    CodeFileFingerprint, CodeImpactRequest, CodeIndexBatch, CodeIndexCheckpoint, CodeIndexSession,
    CodeIndexSnapshot, CodeIndexSummary, CodeIndexTaskRecord, CodeRepositoryRegistration,
    CodeRepositoryReport, CodeRepositoryStatus, CodeRepositoryTotals, CodeRetrievalHit,
    CodeRetrievalRequest, CodeScopeRetentionSummary,
};

use super::{StorageError, StorageFuture};

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

    fn queue_code_index_task(
        &self,
        task: CodeIndexTaskSeed,
    ) -> StorageFuture<'_, CodeIndexTaskRecord>;

    fn claim_code_index_task(
        &self,
        request: CodeIndexTaskClaimRequest,
    ) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

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
}
