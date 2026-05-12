use std::sync::Arc;

use super::*;
use crate::{
    api::{HybridRetrievalRequest, InterfaceKind, RequestContext},
    domain::{
        CodeChunkRecord, CodeFileFingerprint, CodeGraphBatch, CodeGraphCommitReceipt,
        CodeImpactRequest, CodeIndexSnapshot, CodeIndexSummary, CodeReferenceRecord,
        CodeRepositoryRegistration, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest,
        CodeSymbolRecord, CommitReceipt, FreshnessPolicy, GraphMutationBatch, GraphVersion,
        IndexKind, IndexStatus, RetrievalHit, RetrievalMode,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeChunkSearchRequest, CodeGraphStore, CodeImpactChanges, CodeReferenceSearchRequest,
        CodeRepositoryStore, CodeSymbolSearchRequest, GraphInspection, GraphSearchRequest,
        GraphStore, IndexStore, KnowledgeStore, MutationLogEntry, MutationLogStore, StorageError,
        StorageFuture,
    },
};

#[tokio::test]
async fn graph_only_retrieval_bypasses_index_metadata() {
    let service = service_with_store(Arc::new(GraphOnlySearchStore)).await;

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "Rust".to_owned(),
                source_scope: Some(" docs ".to_owned()),
                limit: 5,
                freshness: FreshnessPolicy::GraphOnly,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("graph-only query should not require index metadata");

    assert_eq!(response.retrieval_mode, RetrievalMode::GraphOnly);
    assert_eq!(
        response.degraded_reason.as_deref(),
        Some("graph_only freshness policy selected")
    );
    assert!(response.indexes.is_empty());
    assert_eq!(response.metadata.index_version, None);
    assert_eq!(response.results[0].source_scope, "docs");
}

struct GraphOnlySearchStore;

impl GraphStore for GraphOnlySearchStore {
    fn commit_mutation_batch(
        &self,
        _batch: GraphMutationBatch,
    ) -> StorageFuture<'_, CommitReceipt> {
        unsupported("graph-only fixture does not commit")
    }

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection> {
        Box::pin(async {
            Ok(GraphInspection {
                graph_version: GraphVersion::new(1),
                entity_count: 1,
                evidence_count: 1,
                relation_count: 0,
                claim_count: 0,
                event_count: 0,
                mutation_count: 1,
                code_file_count: 0,
                code_symbol_count: 0,
                code_reference_count: 0,
                code_chunk_count: 0,
                code_parse_status_counts: Default::default(),
            })
        })
    }

    fn search(&self, request: GraphSearchRequest) -> StorageFuture<'_, Vec<RetrievalHit>> {
        Box::pin(async move {
            assert_eq!(request.source_scope.as_deref(), Some("docs"));

            Ok(vec![RetrievalHit {
                evidence_id: "ev-graph-only".to_owned(),
                source_scope: "docs".to_owned(),
                source_path: None,
                content: format!("{} result", request.query),
                entity_labels: Vec::new(),
                retriever_sources: vec![crate::domain::RetrieverSource::GraphEvidence],
                ranking: Vec::new(),
                score: 1.0,
            }])
        })
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        Box::pin(async { Ok(GraphVersion::new(1)) })
    }
}

impl MutationLogStore for GraphOnlySearchStore {
    fn read_after(
        &self,
        _graph_version: GraphVersion,
        _limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>> {
        unsupported("graph-only fixture does not read mutations")
    }
}

impl IndexStore for GraphOnlySearchStore {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
        unsupported("index metadata is unavailable")
    }

    fn mark_refresh_complete(
        &self,
        _kind: IndexKind,
        _graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        unsupported("index metadata is unavailable")
    }
}

impl CodeGraphStore for GraphOnlySearchStore {
    fn commit_code_graph_batch(
        &self,
        _batch: CodeGraphBatch,
    ) -> StorageFuture<'_, CodeGraphCommitReceipt> {
        unsupported("graph-only fixture does not commit code graph facts")
    }

    fn search_code_symbols(
        &self,
        _request: CodeSymbolSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeSymbolRecord>> {
        unsupported("graph-only fixture does not search code symbols")
    }

    fn search_code_references(
        &self,
        _request: CodeReferenceSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeReferenceRecord>> {
        unsupported("graph-only fixture does not search code references")
    }

    fn search_code_chunks(
        &self,
        _request: CodeChunkSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeChunkRecord>> {
        unsupported("graph-only fixture does not search code chunks")
    }
}

macro_rules! unsupported_code_repository_method {
    ($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        fn $name(&self, $($arg: $ty),*) -> StorageFuture<'_, $ret> {
            $(let _ = $arg;)*
            unsupported("code repository storage is unavailable")
        }
    };
}

impl CodeRepositoryStore for GraphOnlySearchStore {
    unsupported_code_repository_method!(upsert_code_repository(registration: CodeRepositoryRegistration) -> CodeRepositoryStatus);
    unsupported_code_repository_method!(code_repository_status(repository: String) -> Option<CodeRepositoryStatus>);
    unsupported_code_repository_method!(code_file_fingerprints(repository_id: String) -> Vec<CodeFileFingerprint>);
    unsupported_code_repository_method!(apply_code_index_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexSummary);
    unsupported_code_repository_method!(search_code(request: CodeRetrievalRequest) -> Vec<CodeRetrievalHit>);
    unsupported_code_repository_method!(analyze_code_impact(request: CodeImpactRequest, changes: CodeImpactChanges) -> Vec<CodeRetrievalHit>);
}

fn unsupported<T: Send + 'static>(message: &'static str) -> StorageFuture<'static, T> {
    Box::pin(async move { Err(StorageError::InvalidInput(message.to_owned())) })
}

async fn service_with_store(store: Arc<dyn KnowledgeStore>) -> RelayKnowledgeService {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
}
