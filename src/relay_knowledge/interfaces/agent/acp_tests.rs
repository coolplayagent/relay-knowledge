use std::{sync::Arc, time::Duration};

use super::*;
use crate::{
    api::{IngestEvidence, IngestRequest},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeChunkRecord, CodeFileFingerprint, CodeGraphBatch, CodeGraphCommitReceipt,
        CodeImpactRequest, CodeIndexSnapshot, CodeIndexSummary, CodeReferenceRecord,
        CodeRepositoryRegistration, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest,
        CodeSymbolRecord, CommitReceipt, GraphMutationBatch, GraphVersion, IndexKind, IndexStatus,
        RetrievalHit,
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
async fn local_acp_prompt_returns_progress_context_artifact_and_audit() {
    let (adapter, service) =
        adapter_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![IngestEvidence {
                    id: Some("ev-acp".to_owned()),
                    source_path: None,
                    span: None,
                    confidence: None,
                    status: None,
                    content: "ACP local sessions retrieve graph context".to_owned(),
                    entity_labels: vec!["ACP".to_owned()],
                }],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");
    let capabilities = adapter.initialize();
    let session = adapter
        .new_session(AcpSessionRequest {
            client_name: Some("fixture-client".to_owned()),
            client_version: Some("0.1.0".to_owned()),
            actor_id: Some("actor-1".to_owned()),
        })
        .expect("session should start");

    let response = adapter
        .prompt(
            &session.session_id,
            AcpPromptRequest {
                prompt: "ignored when structured query is present".to_owned(),
                request_id: Some("turn-1".to_owned()),
                meta: Some(AcpPromptMeta {
                    relay_knowledge: Some(AcpRelayKnowledgePrompt {
                        query: Some("local sessions".to_owned()),
                        source_scope: Some("docs".to_owned()),
                        limit: Some(2),
                        freshness: Some("wait-until-fresh".to_owned()),
                    }),
                }),
            },
        )
        .await;

    assert!(capabilities.meta.relay_knowledge.graph_retrieval);
    assert_eq!(response.stop_reason, AcpStopReason::Completed);
    assert_eq!(
        response
            .context_artifact
            .as_ref()
            .expect("artifact should be present")
            .result
            .runtime_identity
            .protocol,
        AgentProtocolKind::Acp
    );
    assert_eq!(
        response.context_artifact.as_ref().unwrap().result.results[0].evidence_id,
        "ev-acp"
    );
    assert!(
        response
            .updates
            .iter()
            .any(|update| update.message == "freshness checked")
    );

    let audit = adapter.audit_snapshot();
    let event = audit.last().expect("prompt should write audit event");
    assert_eq!(event.operation, "session/prompt");
    assert_eq!(event.source_scope.as_deref(), Some("docs"));
    assert_eq!(event.freshness.as_deref(), Some("wait-until-fresh"));
    assert_eq!(event.result_count, Some(1));
    assert_eq!(event.status, AgentAuditStatus::Completed);
}

#[tokio::test]
async fn local_acp_prompt_can_be_cancelled_and_releases_qos() {
    let (adapter, _service) = adapter_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS", "500"),
        ],
        Arc::new(SlowSearchStore),
    )
    .await;
    let session = adapter
        .new_session(AcpSessionRequest::default())
        .expect("session should start");
    let session_id = session.session_id.clone();
    let prompt_adapter = adapter.clone();
    let handle = tokio::spawn(async move {
        prompt_adapter
            .prompt(
                &session_id,
                AcpPromptRequest {
                    prompt: "slow search".to_owned(),
                    request_id: Some("turn-cancel".to_owned()),
                    meta: Some(AcpPromptMeta {
                        relay_knowledge: Some(AcpRelayKnowledgePrompt {
                            query: Some("slow search".to_owned()),
                            source_scope: Some("docs".to_owned()),
                            limit: Some(1),
                            freshness: Some("allow-stale".to_owned()),
                        }),
                    }),
                },
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(10)).await;

    assert!(adapter.cancel(&session.session_id, "turn-cancel"));
    let response = handle.await.expect("prompt task should finish");

    assert_eq!(response.stop_reason, AcpStopReason::Cancelled);
    assert_eq!(
        response
            .error
            .as_ref()
            .expect("error should be present")
            .error_kind,
        "cancelled"
    );
    assert_eq!(adapter.qos_snapshot().in_flight_requests, 0);
    assert_eq!(
        adapter
            .audit_snapshot()
            .last()
            .expect("cancel should audit")
            .status,
        AgentAuditStatus::Cancelled
    );
}

#[tokio::test]
async fn local_acp_prompt_reports_policy_qos_timeout_and_service_errors() {
    let (adapter, _service) =
        adapter_and_service([("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")]).await;
    let unknown = adapter
        .prompt(
            "missing-session",
            AcpPromptRequest {
                prompt: "anything".to_owned(),
                request_id: None,
                meta: None,
            },
        )
        .await;
    assert_eq!(unknown.stop_reason, AcpStopReason::Failed);
    assert!(unknown.request_id.starts_with("acp-request-"));

    let session = adapter
        .new_session(AcpSessionRequest::default())
        .expect("session should start");
    let invalid = adapter
        .prompt(
            &session.session_id,
            AcpPromptRequest {
                prompt: "missing source scope".to_owned(),
                request_id: Some("turn-invalid".to_owned()),
                meta: None,
            },
        )
        .await;
    assert_eq!(
        invalid.error.as_ref().expect("error").error_kind,
        "invalid_scope"
    );

    let (blocked, _service) = adapter_and_service([
        ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
        ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "1"),
    ])
    .await;
    let blocked_session = blocked
        .new_session(AcpSessionRequest::default())
        .expect("session should start before budget is occupied");
    let policy = blocked.network.current().qos;
    let _occupied = blocked
        .qos
        .admit_request(&policy)
        .expect("test should occupy request budget");
    let rejected = blocked
        .prompt(
            &blocked_session.session_id,
            AcpPromptRequest {
                prompt: "qos".to_owned(),
                request_id: Some("turn-qos".to_owned()),
                meta: Some(AcpPromptMeta {
                    relay_knowledge: Some(AcpRelayKnowledgePrompt {
                        query: Some("qos".to_owned()),
                        source_scope: Some("docs".to_owned()),
                        limit: Some(1),
                        freshness: None,
                    }),
                }),
            },
        )
        .await;
    assert_eq!(
        rejected.error.as_ref().expect("error").error_kind,
        "qos_rejected"
    );

    let (timeout_adapter, _service) = adapter_and_service_with_store(
        [
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs"),
            ("RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS", "1"),
        ],
        Arc::new(SlowSearchStore),
    )
    .await;
    let timeout_session = timeout_adapter
        .new_session(AcpSessionRequest::default())
        .expect("session should start");
    let timed_out = timeout_adapter
        .prompt(
            &timeout_session.session_id,
            scoped_prompt("turn-timeout", "docs", "slow"),
        )
        .await;
    assert_eq!(
        timed_out.error.as_ref().expect("error").error_kind,
        "timeout"
    );

    let (failing_adapter, _service) = adapter_and_service_with_store(
        [("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs")],
        Arc::new(SearchFailStore),
    )
    .await;
    let failing_session = failing_adapter
        .new_session(AcpSessionRequest::default())
        .expect("session should start");
    let failed = failing_adapter
        .prompt(
            &failing_session.session_id,
            scoped_prompt("turn-failed", "docs", "storage"),
        )
        .await;
    assert_eq!(
        failed.error.as_ref().expect("error").error_kind,
        "storage_unavailable"
    );
}

#[test]
fn local_acp_helpers_cover_drop_and_error_mapping_paths() {
    let registry = AcpSessionRegistry::default();
    let (_receiver, registration) = registry.register_request("session", "request".to_owned());
    drop(registration);
    assert!(!registry.cancel_request("session", "request"));

    assert_eq!(
        api_error_kind(ErrorKind::InvalidArgument),
        AgentAdapterErrorKind::InvalidArgument
    );
    assert_eq!(
        api_error_kind(ErrorKind::Timeout),
        AgentAdapterErrorKind::Timeout
    );
    assert_eq!(
        api_error_kind(ErrorKind::Internal),
        AgentAdapterErrorKind::Internal
    );
    assert_eq!(
        qos_error(RejectReason::ConnectionBudgetExceeded).kind,
        AgentAdapterErrorKind::QosRejected
    );
    assert_eq!(
        qos_error(RejectReason::QueueBudgetExceeded).message,
        "queue budget exhausted"
    );
}

async fn adapter_and_service<const N: usize>(
    pairs: [(&str, &str); N],
) -> (LocalAcpSessionAdapter, RelayKnowledgeService) {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    adapter_and_service_with_store(pairs, store).await
}

async fn adapter_and_service_with_store<const N: usize>(
    pairs: [(&str, &str); N],
    store: Arc<dyn crate::storage::KnowledgeStore>,
) -> (LocalAcpSessionAdapter, RelayKnowledgeService) {
    let mut base = vec![
        ("HOME", "/home/alice"),
        ("TMPDIR", "/tmp"),
        ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
    ];
    base.extend(pairs);
    let environment =
        EnvironmentConfig::from_pairs(PlatformKind::Unix, base).expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let service = RelayKnowledgeService::with_store(runtime.clone(), store);
    let adapter = LocalAcpSessionAdapter::new(
        service.clone(),
        runtime.network.clone(),
        runtime.agent.clone(),
    );

    (adapter, service)
}

fn scoped_prompt(request_id: &str, source_scope: &str, query: &str) -> AcpPromptRequest {
    AcpPromptRequest {
        prompt: query.to_owned(),
        request_id: Some(request_id.to_owned()),
        meta: Some(AcpPromptMeta {
            relay_knowledge: Some(AcpRelayKnowledgePrompt {
                query: Some(query.to_owned()),
                source_scope: Some(source_scope.to_owned()),
                limit: Some(1),
                freshness: Some("allow-stale".to_owned()),
            }),
        }),
    }
}

struct SlowSearchStore;

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
    unsupported_code_repository_method!(code_file_fingerprints(repository_id: String) -> Vec<CodeFileFingerprint>);
    unsupported_code_repository_method!(apply_code_index_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexSummary);
    unsupported_code_repository_method!(search_code(request: CodeRetrievalRequest) -> Vec<CodeRetrievalHit>);
    unsupported_code_repository_method!(analyze_code_impact(request: CodeImpactRequest, changes: CodeImpactChanges) -> Vec<CodeRetrievalHit>);
}

struct SearchFailStore;

impl GraphStore for SearchFailStore {
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
            Err(StorageError::InvalidInput(
                "search storage unavailable".to_owned(),
            ))
        })
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        Box::pin(async { Ok(GraphVersion::new(1)) })
    }
}

impl MutationLogStore for SearchFailStore {
    fn read_after(
        &self,
        _graph_version: GraphVersion,
        _limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

impl IndexStore for SearchFailStore {
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

impl CodeGraphStore for SearchFailStore {
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

impl CodeRepositoryStore for SearchFailStore {
    unsupported_code_repository_method!(upsert_code_repository(registration: CodeRepositoryRegistration) -> CodeRepositoryStatus);
    unsupported_code_repository_method!(code_repository_status(repository: String) -> Option<CodeRepositoryStatus>);
    unsupported_code_repository_method!(code_file_fingerprints(repository_id: String) -> Vec<CodeFileFingerprint>);
    unsupported_code_repository_method!(apply_code_index_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexSummary);
    unsupported_code_repository_method!(search_code(request: CodeRetrievalRequest) -> Vec<CodeRetrievalHit>);
    unsupported_code_repository_method!(analyze_code_impact(request: CodeImpactRequest, changes: CodeImpactChanges) -> Vec<CodeRetrievalHit>);
}
