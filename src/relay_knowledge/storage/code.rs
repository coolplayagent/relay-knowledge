//! Storage contracts for code repository indexes.

use crate::domain::{
    CodeFileFingerprint, CodeImpactRequest, CodeIndexSnapshot, CodeIndexSummary,
    CodeRepositoryRegistration, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest,
};

use super::StorageFuture;

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

    fn code_file_fingerprints(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Vec<CodeFileFingerprint>>;

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary>;

    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>>;

    fn analyze_code_impact(
        &self,
        request: CodeImpactRequest,
        changed_paths: Vec<String>,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>>;
}
