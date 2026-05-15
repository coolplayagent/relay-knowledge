use std::sync::Arc;

use crate::{
    api::{
        IngestEvidence, IngestEvidenceExtraction, IngestRequest, InterfaceKind,
        ProposalDecisionApiRequest, ProposalListApiRequest, RequestContext, ServicePlanRequest,
        WorkerRunRequest, WorkerStatusRequest,
    },
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        EvidenceModality, ProposalState, ServiceManagerAction, ServiceOperatorState, WorkerKind,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{KnowledgeStore, SqliteGraphStore},
};

#[tokio::test]
async fn service_status_reports_current_graph_version() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-service",
                "Service status tracks graph versions",
                Vec::new(),
            )]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .service_status(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-service",
            "trace-service",
        ))
        .await
        .expect("service status should load");

    assert_eq!(response.metadata.graph_version, 1);
    assert_eq!(response.metadata.trace_id, "trace-service");
}

#[tokio::test]
async fn service_status_hides_mcp_subcapabilities_when_mcp_runtime_is_disabled() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ("RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED", "false"),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let service = RelayKnowledgeService::with_store(runtime, store as Arc<dyn KnowledgeStore>);

    let response = service
        .service_status(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-service",
            "trace-service",
        ))
        .await
        .expect("service status should load");

    assert!(!response.agent_protocols.mcp_streamable_http_enabled);
    assert!(!response.agent_protocols.mcp_resources_enabled);
    assert!(!response.agent_protocols.mcp_prompts_enabled);
}

#[tokio::test]
async fn multimodal_ingest_queues_worker_and_accepts_manual_proposal() {
    let service = service_with_memory_store().await;
    let mut image = ingest_evidence("image-1", "image asset", Vec::new());
    image.extraction = Some(IngestEvidenceExtraction {
        modality: EvidenceModality::ImageAsset,
        source_uri: Some("file:///tmp/image.png".to_owned()),
        source_hash: None,
        media_hash: Some("sha256:image".to_owned()),
        extractor: Some("fixture".to_owned()),
        extractor_version: Some("1".to_owned()),
        observed_at: Some("2026-05-13T00:00:00Z".to_owned()),
        parent_evidence_id: None,
        layout_region: None,
        embedding_model: None,
        embedding_dimension: None,
        diagnostic: None,
    });
    service
        .ingest(
            ingest_request(vec![image]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("image ingest should queue workers");

    let status = service
        .worker_status(
            WorkerStatusRequest {
                kind: Some(WorkerKind::Ocr),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-worker-status", "trace"),
        )
        .await
        .expect("worker status should load");

    assert_eq!(status.workers[0].queue_depth, 1);

    let run = service
        .run_worker_once(
            WorkerRunRequest {
                kind: Some(WorkerKind::Ocr),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-worker", "trace"),
        )
        .await
        .expect("worker should create a proposal");

    assert_eq!(run.proposals.len(), 1);
    assert_eq!(run.proposals[0].state, ProposalState::Proposed);

    let accepted = service
        .accept_proposal(
            run.proposals[0].proposal_id.clone(),
            ProposalDecisionApiRequest {
                actor: "tester".to_owned(),
                reason: Some("looks correct".to_owned()),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-accept", "trace"),
        )
        .await
        .expect("proposal should commit through graph mutation");

    assert_eq!(accepted.proposal.state, ProposalState::Accepted);
    assert_eq!(accepted.receipt.expect("receipt").evidence_count, 1);
}

#[tokio::test]
async fn service_plan_and_operator_state_are_shared_api_surfaces() {
    let service = service_with_memory_store().await;

    let plan = service
        .service_plan(
            ServicePlanRequest {
                action: ServiceManagerAction::Install,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-plan", "trace"),
        )
        .await
        .expect("plan should render");

    assert!(plan.plan.definition.contains("relay-knowledge"));
    assert!(!plan.plan.install_command.is_empty());

    let paused = service
        .set_service_operator_state(
            ServiceOperatorState::Paused,
            RequestContext::with_ids(InterfaceKind::Cli, "req-pause", "trace"),
        )
        .await
        .expect("operator should pause");

    assert_eq!(paused.operator.state, ServiceOperatorState::Paused);

    let proposals = service
        .list_proposals(
            ProposalListApiRequest {
                state: Some(ProposalState::Proposed),
                limit: 10,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-proposals", "trace"),
        )
        .await
        .expect("proposal list should load");

    assert!(proposals.proposals.is_empty());
}

#[tokio::test]
async fn extractor_worker_structured_facts_remain_proposed_with_provenance() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("worker listener should bind");
    let endpoint = format!("http://{}/extract", listener.local_addr().expect("addr"));
    let worker = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("worker request");
        let mut buffer = vec![0; 4096];
        let count = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer)
            .await
            .expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..count]);

        assert!(request.contains("\"contract_version\":2"));
        assert!(request.contains("\"structured_facts_default_status\":\"proposed\""));
        assert!(request.contains("\"request_timeout_ms\""));

        let body = serde_json::json!({
            "title": "LLM SPO extraction proposal",
            "summary": "Model extracted one relation candidate",
            "confidence_basis_points": 8600,
            "provenance": {
                "producer": "llm_spo_extraction",
                "provider": "fixture-provider",
                "model": "fixture-model",
                "prompt_id": "relay.extract.spo",
                "prompt_version": "1",
                "schema_version": "worker-proposal.v2",
                "input_source_hash": "sha256:fixture",
                "input_fact_ids": ["ev-extract"],
                "stale_when": ["source hash changes"],
                "budget_notes": ["candidate_limit=1"]
            },
            "ingest_request": {
                "source_scope": "docs",
                "relations": [{
                    "id": "rel-extracted",
                    "source_entity_label": "relay-knowledge",
                    "relation_type": "uses",
                    "target_entity_label": "proposal review",
                    "evidence_ids": ["ev-extract"],
                    "status": "accepted"
                }]
            }
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes())
            .await
            .expect("response should write");
    });
    let service = service_with_worker_endpoint(&endpoint).await;
    service
        .ingest(
            ingest_request(vec![ingest_evidence(
                "ev-extract",
                "relay-knowledge uses proposal review",
                vec!["relay-knowledge".to_owned(), "proposal review".to_owned()],
            )]),
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should queue extractor worker");

    let run = service
        .run_worker_once(
            WorkerRunRequest {
                kind: Some(WorkerKind::Extractor),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-worker", "trace-worker"),
        )
        .await
        .expect("extractor worker should create a proposal");

    let proposal = run.proposals.first().expect("proposal should be returned");
    let payload = serde_json::from_str::<IngestRequest>(&proposal.payload_json)
        .expect("proposal payload should remain an ingest request");

    assert_eq!(proposal.provenance.producer, "llm_spo_extraction");
    assert_eq!(
        proposal.provenance.prompt_id.as_deref(),
        Some("relay.extract.spo")
    );
    assert_eq!(proposal.kind.as_str(), "relation");
    assert_eq!(
        payload.relations[0].status,
        Some(crate::domain::FactStatus::Proposed)
    );
    worker.await.expect("worker server should finish");
}

async fn service_with_memory_store() -> RelayKnowledgeService {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
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

    RelayKnowledgeService::with_store(runtime, store as Arc<dyn KnowledgeStore>)
}

async fn service_with_worker_endpoint(endpoint: &str) -> RelayKnowledgeService {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ("RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT", endpoint),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store as Arc<dyn KnowledgeStore>)
}

fn ingest_request(evidence: Vec<IngestEvidence>) -> IngestRequest {
    IngestRequest {
        source_scope: "docs".to_owned(),
        evidence,
        relations: Vec::new(),
        claims: Vec::new(),
        events: Vec::new(),
    }
}

fn ingest_evidence(
    id: impl Into<String>,
    content: impl Into<String>,
    entity_labels: Vec<String>,
) -> IngestEvidence {
    IngestEvidence {
        id: Some(id.into()),
        source_path: None,
        span: None,
        confidence: None,
        status: None,
        content: content.into(),
        entity_labels,
        extraction: None,
    }
}
