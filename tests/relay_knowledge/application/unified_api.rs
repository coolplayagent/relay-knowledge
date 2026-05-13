use std::sync::Arc;

use relay_knowledge::{
    api::{
        GraphInspectionRequest, HybridRetrievalRequest, IngestClaim, IngestEvent, IngestEvidence,
        IngestRelation, IngestRequest, InterfaceKind, RequestContext,
    },
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeChunkRecord, CodeFileFingerprint, CodeGraphBatch, CodeGraphCommitReceipt,
        CodeImpactRequest, CodeIndexSnapshot, CodeIndexSummary, CodeReferenceRecord,
        CodeRepositoryRegistration, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest,
        CodeSymbolRecord, CommitReceipt, ConfidenceScore, ContextGraphFactKind, EvidenceSpan,
        FactStatus, FreshnessPolicy, GraphMutationBatch, GraphVersion, GraphVersionRange,
        IndexKind, IndexStatus, RetrievalBackendState, RetrievalHit, RetrieverSource,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeChunkSearchRequest, CodeGraphStore, CodeImpactChanges, CodeReferenceSearchRequest,
        CodeRepositoryStore, CodeSymbolSearchRequest, GraphInspection, GraphSearchRequest,
        GraphStore, IndexStore, MutationLogEntry, MutationLogStore, SqliteGraphStore, StorageError,
        StorageFuture,
    },
};

#[tokio::test]
async fn cli_and_web_can_use_the_same_application_service() {
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
    let service = service_with_store(runtime, Arc::new(memory_store()));
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-unified".to_owned()),
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "Unified API status reports shared graph version".to_owned(),
                    entity_labels: Vec::new(),
                }],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");
    let cli_context = RequestContext::with_ids(InterfaceKind::Cli, "req-cli", "trace-cli");
    let web_context = RequestContext::with_ids(InterfaceKind::Web, "req-web", "trace-web");

    let cli_response = service
        .project_status(cli_context)
        .await
        .expect("CLI status should load");
    let web_response = service
        .project_status(web_context)
        .await
        .expect("Web status should load");

    assert_eq!(cli_response.project_name, "relay-knowledge");
    assert_eq!(web_response.project_name, "relay-knowledge");
    assert_eq!(cli_response.metadata.graph_version, 1);
    assert_eq!(web_response.metadata.graph_version, 1);
    assert_eq!(cli_response.metadata.trace_id, "trace-cli");
    assert_eq!(web_response.metadata.trace_id, "trace-web");
    assert_eq!(cli_response.runtime.config_dir, "/srv/relay/config");
    assert_eq!(web_response.runtime.http_bind, "127.0.0.1:8791");
}

#[tokio::test]
async fn fallback_evidence_id_does_not_depend_on_batch_position() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![
                    IngestEvidence {
                        id: None,
                        source_path: None,
                        span: None,
                        confidence: None,
                        status: None,
                        content: "Alpha evidence".to_owned(),
                        entity_labels: Vec::new(),
                    },
                    IngestEvidence {
                        id: None,
                        source_path: None,
                        span: None,
                        confidence: None,
                        status: None,
                        content: "Beta evidence".to_owned(),
                        entity_labels: Vec::new(),
                    },
                ],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-batch", "trace-batch"),
        )
        .await
        .expect("batch ingest should succeed");
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: None,
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "Beta evidence".to_owned(),
                    entity_labels: Vec::new(),
                }],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-single", "trace-single"),
        )
        .await
        .expect("single ingest should update existing evidence");

    let graph = service
        .inspect_graph(
            GraphInspectionRequest { source_scope: None },
            RequestContext::with_ids(InterfaceKind::Cli, "req-graph", "trace-graph"),
        )
        .await
        .expect("graph should inspect");

    assert_eq!(graph.graph.graph_version, GraphVersion::new(2));
    assert_eq!(graph.graph.evidence_count, 2);
}

#[tokio::test]
async fn fallback_evidence_id_uses_trimmed_content() {
    let service = service_with_memory_store().await;
    for content in ["Rust graph idempotency", " Rust graph idempotency "] {
        service
            .ingest(
                IngestRequest {
                    source_scope: "docs".to_owned(),
                    evidence: vec![IngestEvidence {
                        id: None,
                        source_path: None,
                        span: None,
                        confidence: None,
                        status: None,
                        content: content.to_owned(),
                        entity_labels: Vec::new(),
                    }],
                    relations: Vec::new(),
                    claims: Vec::new(),
                    events: Vec::new(),
                },
                RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
            )
            .await
            .expect("ingest should succeed");
    }

    let graph = service
        .inspect_graph(
            GraphInspectionRequest { source_scope: None },
            RequestContext::with_ids(InterfaceKind::Cli, "req-graph", "trace-graph"),
        )
        .await
        .expect("graph should inspect");

    assert_eq!(graph.graph.graph_version, GraphVersion::new(2));
    assert_eq!(graph.graph.evidence_count, 1);
}

#[tokio::test]
async fn fallback_evidence_id_includes_source_location_when_present() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![
                    IngestEvidence {
                        id: None,
                        source_path: Some("docs/a.md".to_owned()),
                        span: Some(EvidenceSpan::new(0, 12, 1, 1).expect("span")),
                        confidence: None,
                        status: None,
                        content: "Repeated evidence".to_owned(),
                        entity_labels: Vec::new(),
                    },
                    IngestEvidence {
                        id: None,
                        source_path: Some("docs/b.md".to_owned()),
                        span: Some(EvidenceSpan::new(0, 12, 1, 1).expect("span")),
                        confidence: None,
                        status: None,
                        content: "Repeated evidence".to_owned(),
                        entity_labels: Vec::new(),
                    },
                ],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("same content with different source locations should ingest");

    let graph = service
        .inspect_graph(
            GraphInspectionRequest { source_scope: None },
            RequestContext::with_ids(InterfaceKind::Cli, "req-graph", "trace-graph"),
        )
        .await
        .expect("graph should inspect");

    assert_eq!(graph.graph.evidence_count, 2);
}

#[tokio::test]
async fn ingest_reports_partial_success_when_index_refresh_fails_after_commit() {
    let service = service_with_memory_store_type(Arc::new(RefreshFailStore::default())).await;

    let response = service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-partial".to_owned()),
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "Committed before refresh failure".to_owned(),
                    entity_labels: Vec::new(),
                }],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("committed ingest should return partial success");

    assert_eq!(response.receipt.graph_version, GraphVersion::new(1));
    assert!(response.metadata.stale);
    assert!(response.indexes.is_empty());
    assert_eq!(
        response.index_refresh_error.as_deref(),
        Some("invalid storage input: index refresh task storage is unavailable")
    );
}

#[tokio::test]
async fn structured_facts_and_backend_statuses_reach_context_pack() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-rich".to_owned()),
                    source_path: Some("docs/phase-1.md".to_owned()),
                    span: Some(EvidenceSpan::new(0, 31, 1, 1).expect("span")),
                    confidence: Some(ConfidenceScore::from_ratio(0.95).expect("confidence")),
                    status: Some(FactStatus::Proposed),
                    content: "relay-knowledge uses BM25 retrieval".to_owned(),
                    entity_labels: vec!["relay-knowledge".to_owned(), "BM25".to_owned()],
                }],
                relations: vec![IngestRelation {
                    id: "rel-rich".to_owned(),
                    source_entity_label: "relay-knowledge".to_owned(),
                    relation_type: "uses".to_owned(),
                    target_entity_label: "BM25".to_owned(),
                    evidence_ids: vec!["ev-rich".to_owned()],
                    confidence: Some(ConfidenceScore::from_ratio(0.9).expect("confidence")),
                    status: Some(FactStatus::Accepted),
                    version_range: Some(
                        GraphVersionRange::new(GraphVersion::new(1), None).expect("range"),
                    ),
                }],
                claims: vec![IngestClaim {
                    id: "claim-rich".to_owned(),
                    subject_entity_label: "relay-knowledge".to_owned(),
                    predicate: "retrieval_layer".to_owned(),
                    object: "BM25".to_owned(),
                    evidence_ids: vec!["ev-rich".to_owned()],
                    confidence: Some(ConfidenceScore::from_ratio(0.82).expect("confidence")),
                    status: Some(FactStatus::Superseded),
                    version_range: Some(
                        GraphVersionRange::new(GraphVersion::new(1), None).expect("range"),
                    ),
                }],
                events: vec![IngestEvent {
                    id: "event-rich".to_owned(),
                    event_type: "indexed".to_owned(),
                    entity_labels: vec!["relay-knowledge".to_owned(), "BM25".to_owned()],
                    occurred_at: Some("2026-05-12T00:00:00Z".to_owned()),
                    evidence_ids: vec!["ev-rich".to_owned()],
                    confidence: Some(ConfidenceScore::from_ratio(0.75).expect("confidence")),
                    status: Some(FactStatus::Rejected),
                    version_range: Some(
                        GraphVersionRange::new(GraphVersion::new(1), None).expect("range"),
                    ),
                }],
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-rich", "trace-rich"),
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "BM25".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 5,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Web, "req-query", "trace-query"),
        )
        .await
        .expect("query should succeed");

    let item = response
        .context_pack
        .items
        .first()
        .expect("context item should exist");
    assert_eq!(item.source_path.as_deref(), Some("docs/phase-1.md"));
    assert!(item.source_span.is_some());
    assert!(item.entities.iter().any(|entity| entity.label == "BM25"));
    assert_eq!(item.graph_facts[0].kind, ContextGraphFactKind::Relation);
    assert_eq!(item.graph_facts[0].predicate, "uses");
    let claim = item
        .graph_facts
        .iter()
        .find(|fact| fact.fact_id == "claim-rich")
        .expect("claim fact should be attached");
    assert_eq!(claim.kind, ContextGraphFactKind::Claim);
    assert_eq!(claim.status, FactStatus::Superseded);
    assert_eq!(claim.confidence.basis_points, 8_200);
    let event = item
        .graph_facts
        .iter()
        .find(|fact| fact.fact_id == "event-rich")
        .expect("event fact should be attached");
    assert_eq!(event.kind, ContextGraphFactKind::Event);
    assert_eq!(event.subject, "BM25, relay-knowledge");
    assert_eq!(event.object.as_deref(), Some("2026-05-12T00:00:00Z"));
    assert!(response.backend_statuses.iter().any(|status| {
        status.source == RetrieverSource::Semantic
            && status.state == RetrievalBackendState::Unavailable
            && status.scope_post_filter
    }));
    assert!(
        response.budget_used.context_bytes
            > response
                .results
                .iter()
                .map(|hit| hit.content.len())
                .sum::<usize>()
    );
    assert!(
        response
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("semantic/vector"))
    );
}

#[derive(Default)]
struct RefreshFailStore {
    version: std::sync::Mutex<u64>,
}

impl GraphStore for RefreshFailStore {
    fn commit_mutation_batch(&self, batch: GraphMutationBatch) -> StorageFuture<'_, CommitReceipt> {
        Box::pin(async move {
            let mut version = self
                .version
                .lock()
                .map_err(|_| StorageError::LockPoisoned)?;
            *version += 1;
            Ok(CommitReceipt {
                graph_version: GraphVersion::new(*version),
                evidence_count: batch.evidence.len(),
                entity_count: 0,
                relation_count: batch.relations.len(),
                claim_count: batch.claims.len(),
                event_count: batch.events.len(),
            })
        })
    }

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection> {
        Box::pin(async {
            let version = *self
                .version
                .lock()
                .map_err(|_| StorageError::LockPoisoned)?;
            Ok(GraphInspection {
                graph_version: GraphVersion::new(version),
                entity_count: 0,
                evidence_count: 0,
                relation_count: 0,
                claim_count: 0,
                event_count: 0,
                mutation_count: usize::from(version > 0),
                code_file_count: 0,
                code_symbol_count: 0,
                code_reference_count: 0,
                code_chunk_count: 0,
                code_parse_status_counts: Default::default(),
            })
        })
    }

    fn search(&self, _request: GraphSearchRequest) -> StorageFuture<'_, Vec<RetrievalHit>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        Box::pin(async {
            let version = *self
                .version
                .lock()
                .map_err(|_| StorageError::LockPoisoned)?;
            Ok(GraphVersion::new(version))
        })
    }
}

impl MutationLogStore for RefreshFailStore {
    fn read_after(
        &self,
        _graph_version: GraphVersion,
        _limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

impl IndexStore for RefreshFailStore {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn mark_refresh_complete(
        &self,
        _kind: IndexKind,
        _graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "index metadata unavailable".to_owned(),
            ))
        })
    }
}

impl CodeGraphStore for RefreshFailStore {
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

macro_rules! unsupported_code_method {
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

impl CodeRepositoryStore for RefreshFailStore {
    unsupported_code_method!(upsert_code_repository(registration: CodeRepositoryRegistration) -> CodeRepositoryStatus);
    unsupported_code_method!(code_repository_status(repository: String) -> Option<CodeRepositoryStatus>);
    unsupported_code_method!(code_file_fingerprints(repository_id: String) -> Vec<CodeFileFingerprint>);
    unsupported_code_method!(apply_code_index_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexSummary);
    unsupported_code_method!(search_code(request: CodeRetrievalRequest) -> Vec<CodeRetrievalHit>);
    unsupported_code_method!(analyze_code_impact(request: CodeImpactRequest, changes: CodeImpactChanges) -> Vec<CodeRetrievalHit>);
}

async fn service_with_memory_store() -> RelayKnowledgeService {
    service_with_memory_store_type(Arc::new(memory_store())).await
}

async fn service_with_memory_store_type(
    store: Arc<dyn relay_knowledge::storage::KnowledgeStore>,
) -> RelayKnowledgeService {
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

    service_with_store(runtime, store)
}

fn service_with_store(
    runtime: RuntimeConfiguration,
    store: Arc<dyn relay_knowledge::storage::KnowledgeStore>,
) -> RelayKnowledgeService {
    RelayKnowledgeService::with_store(runtime, store)
}

fn memory_store() -> SqliteGraphStore {
    SqliteGraphStore::open_in_memory().expect("store should open")
}
