use std::time::Duration;

use crate::{
    domain::{
        CodeChunkRecord, CodeFileFingerprint, CodeGraphBatch, CodeGraphCommitReceipt,
        CodeImpactRequest, CodeIndexSnapshot, CodeIndexSummary, CodeReferenceRecord,
        CodeRepositoryRegistration, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest,
        CodeSymbolRecord, CommitReceipt, GraphMutationBatch, GraphVersion, IndexKind, IndexStatus,
        RetrievalHit,
    },
    storage::{
        CodeChunkSearchRequest, CodeGraphStore, CodeImpactChanges, CodeReferenceSearchRequest,
        CodeRepositoryStore, CodeSymbolSearchRequest, GraphInspection, GraphSearchRequest,
        GraphStore, IndexStore, MutationLogEntry, MutationLogStore, StorageError, StorageFuture,
    },
};

pub(super) struct SlowSearchStore;

impl GraphStore for SlowSearchStore {
    fn commit_mutation_batch(
        &self,
        _batch: GraphMutationBatch,
    ) -> StorageFuture<'_, CommitReceipt> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "mutation storage unavailable".to_owned(),
            ))
        })
    }

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(GraphInspection {
                graph_version: GraphVersion::new(1),
                entity_count: 0,
                evidence_count: 0,
                relation_count: 0,
                claim_count: 0,
                event_count: 0,
                mutation_count: 0,
                code_file_count: 0,
                code_symbol_count: 0,
                code_reference_count: 0,
                code_chunk_count: 0,
                code_parse_status_counts: Default::default(),
            })
        })
    }

    fn search(&self, _request: GraphSearchRequest) -> StorageFuture<'_, Vec<RetrievalHit>> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(Vec::new())
        })
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        Box::pin(async { Ok(GraphVersion::new(1)) })
    }
}

impl MutationLogStore for SlowSearchStore {
    fn read_after(
        &self,
        _graph_version: GraphVersion,
        _limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

impl IndexStore for SlowSearchStore {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
        Box::pin(async { Ok(IndexKind::ALL.into_iter().map(IndexStatus::empty).collect()) })
    }

    fn mark_refresh_complete(
        &self,
        kind: IndexKind,
        _graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        Box::pin(async move { Ok(IndexStatus::empty(kind)) })
    }
}

impl CodeGraphStore for SlowSearchStore {
    fn commit_code_graph_batch(
        &self,
        _batch: CodeGraphBatch,
    ) -> StorageFuture<'_, CodeGraphCommitReceipt> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code graph storage unavailable".to_owned(),
            ))
        })
    }

    fn search_code_symbols(
        &self,
        _request: CodeSymbolSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeSymbolRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn search_code_references(
        &self,
        _request: CodeReferenceSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeReferenceRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn search_code_chunks(
        &self,
        _request: CodeChunkSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeChunkRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

macro_rules! unsupported_code_repository_method {
    ($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        fn $name(&self, $($arg: $ty),*) -> StorageFuture<'_, $ret> {
            $(let _ = $arg;)*
            Box::pin(async {
                Err(StorageError::InvalidInput(
                    "code repository storage unavailable".to_owned(),
                ))
            })
        }
    };
}

impl CodeRepositoryStore for SlowSearchStore {
    unsupported_code_repository_method!(upsert_code_repository(registration: CodeRepositoryRegistration) -> CodeRepositoryStatus);
    unsupported_code_repository_method!(code_repository_status(repository: String) -> Option<CodeRepositoryStatus>);
    unsupported_code_repository_method!(code_repository_scope_status(repository: String, resolved_commit_sha: String, path_filters: Vec<String>, language_filters: Vec<String>) -> Option<CodeRepositoryStatus>);
    unsupported_code_repository_method!(code_file_fingerprints(repository_id: String) -> Vec<CodeFileFingerprint>);
    unsupported_code_repository_method!(apply_code_index_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexSummary);
    unsupported_code_repository_method!(search_code(request: CodeRetrievalRequest) -> Vec<CodeRetrievalHit>);
    unsupported_code_repository_method!(analyze_code_impact(request: CodeImpactRequest, changes: CodeImpactChanges) -> Vec<CodeRetrievalHit>);
}
