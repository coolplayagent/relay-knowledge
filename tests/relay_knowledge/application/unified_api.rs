use std::sync::Arc;

use relay_knowledge::{
    api::{GraphInspectionRequest, IngestEvidence, IngestRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CommitReceipt, GraphMutationBatch, GraphVersion, IndexKind, IndexStatus, RetrievalHit,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        GraphInspection, GraphSearchRequest, GraphStore, IndexStore, MutationLogEntry,
        MutationLogStore, SqliteGraphStore, StorageError, StorageFuture,
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
                    content: "Unified API status reports shared graph version".to_owned(),
                    entity_labels: Vec::new(),
                }],
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
                        content: "Alpha evidence".to_owned(),
                        entity_labels: Vec::new(),
                    },
                    IngestEvidence {
                        id: None,
                        content: "Beta evidence".to_owned(),
                        entity_labels: Vec::new(),
                    },
                ],
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
                    content: "Beta evidence".to_owned(),
                    entity_labels: Vec::new(),
                }],
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
                        content: content.to_owned(),
                        entity_labels: Vec::new(),
                    }],
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
async fn ingest_reports_partial_success_when_index_refresh_fails_after_commit() {
    let service = service_with_memory_store_type(Arc::new(RefreshFailStore::default())).await;

    let response = service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-partial".to_owned()),
                    content: "Committed before refresh failure".to_owned(),
                    entity_labels: Vec::new(),
                }],
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
        Some("invalid storage input: index metadata unavailable")
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
                mutation_count: usize::from(version > 0),
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
