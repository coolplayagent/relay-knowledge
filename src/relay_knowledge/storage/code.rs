//! Storage contracts for code repository indexes.

use crate::domain::{
    CodeFileFingerprint, CodeImpactRequest, CodeIndexBatch, CodeIndexCheckpoint, CodeIndexSession,
    CodeIndexSnapshot, CodeIndexSummary, CodeRepositoryRegistration, CodeRepositoryReport,
    CodeRepositoryStatus, CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest,
};

use super::{StorageError, StorageFuture};

/// Diff-derived inputs used to seed code impact expansion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeImpactChanges {
    pub paths: Vec<String>,
    pub deleted_symbol_names: Vec<String>,
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
