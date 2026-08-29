use crate::domain::{CodeFileFingerprint, IndexedRepositoryDocument};

use super::super::{StorageError, StorageFuture};

/// Bounded indexed-source reads used by incremental planning and fallbacks.
pub trait CodeIndexSourceStore: Send + Sync {
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
}
