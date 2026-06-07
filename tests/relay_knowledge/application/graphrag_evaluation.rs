use std::sync::Arc;

use relay_knowledge::{
    api::{
        HybridRetrievalRequest, IngestEvent, IngestEvidence, IngestRelation, IngestRequest,
        InterfaceKind, RequestContext,
    },
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeRetrievalHit, CodeRetrievalLayer, EvidenceRecord, FactStatus, FreshnessPolicy,
        GraphMutationBatch, RepositoryCodeRange, SourceScope,
    },
    env::{EnvironmentConfig, PlatformKind},
    evaluation::{EvaluationObservation, evaluate_suite, phase4_fixture_cases},
    storage::{GraphStore, KnowledgeStore, SqliteGraphStore},
};

#[tokio::test]
async fn graphrag_fixture_dataset_scores_phase4_cases() {
    let (service, store) = service_with_store().await;
    ingest_phase4_fixture(&service).await;
    let cases = phase4_fixture_cases().expect("fixture cases should validate");

    let mut observations = Vec::new();
    for case in &cases[..4] {
        observations
            .push(observe_retrieval(&service, &case.query, FreshnessPolicy::WaitUntilFresh).await);
    }
    commit_stale_fixture(&store).await;
    observations
        .push(observe_retrieval(&service, &cases[4].query, FreshnessPolicy::AllowStale).await);
    observations
        .push(observe_retrieval(&service, &cases[5].query, FreshnessPolicy::WaitUntilFresh).await);
    observations.push(EvaluationObservation::from_code_impact(&[code_hit()]));

    let report = evaluate_suite(&cases, &observations).expect("suite should score");

    assert!(report.passed, "{:?}", report.results);
}

async fn ingest_phase4_fixture(service: &RelayKnowledgeService) {
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![
                    evidence(
                        "ev-exact",
                        "Exact fact async SQLite retrieval proves BM25 grounding",
                        &["SQLite"],
                    ),
                    evidence(
                        "ev-path",
                        "GraphRAG path retrieval links schema traversal to vector recall",
                        &["GraphRAG", "Vector"],
                    ),
                    evidence(
                        "ev-temporal",
                        "Relay release happened during the 2026 retrieval timeline",
                        &["Relay"],
                    ),
                    rejected_evidence(
                        "ev-rejected",
                        "Rejected only context must not ground answers",
                    ),
                    evidence(
                        "ev-rust-language",
                        "Rust language ownership and async services",
                        &["Rust"],
                    ),
                    evidence(
                        "ev-rust-material",
                        "Rust material inspection in maintenance notes",
                        &["Rust"],
                    ),
                ],
                relations: vec![IngestRelation {
                    id: "rel-path".to_owned(),
                    source_entity_label: "GraphRAG".to_owned(),
                    relation_type: "uses".to_owned(),
                    target_entity_label: "Vector".to_owned(),
                    evidence_ids: vec!["ev-path".to_owned()],
                    confidence: None,
                    status: None,
                    version_range: None,
                }],
                claims: Vec::new(),
                events: vec![IngestEvent {
                    id: "event-temporal".to_owned(),
                    event_type: "release".to_owned(),
                    entity_labels: vec!["Relay".to_owned()],
                    occurred_at: Some("2026-05-13".to_owned()),
                    evidence_ids: vec!["ev-temporal".to_owned()],
                    confidence: None,
                    status: None,
                    version_range: None,
                }],
            },
            context("ingest-fixture"),
        )
        .await
        .expect("fixture should ingest");
}

async fn commit_stale_fixture(store: &SqliteGraphStore) {
    let evidence = EvidenceRecord::new(
        "ev-stale",
        SourceScope::parse("docs").expect("scope should parse"),
        "Stale index refresh observation keeps metadata honest",
        vec!["Stale".to_owned()],
    )
    .expect("stale evidence should validate");

    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("stale commit should succeed");
}

async fn observe_retrieval(
    service: &RelayKnowledgeService,
    query: &str,
    freshness: FreshnessPolicy,
) -> EvaluationObservation {
    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: query.to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 10,
                freshness,
            },
            context(query),
        )
        .await
        .expect("retrieval should succeed");

    EvaluationObservation::from_retrieval(&response)
}

async fn service_with_store() -> (RelayKnowledgeService, Arc<SqliteGraphStore>) {
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
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service_store: Arc<dyn KnowledgeStore> = store.clone();
    let service = RelayKnowledgeService::with_store(runtime, service_store);

    (service, store)
}

fn evidence(id: &str, content: &str, labels: &[&str]) -> IngestEvidence {
    IngestEvidence {
        id: Some(id.to_owned()),
        source_path: None,
        span: None,
        confidence: None,
        status: None,
        content: content.to_owned(),
        entity_labels: labels.iter().map(|label| (*label).to_owned()).collect(),
        extraction: None,
    }
}

fn rejected_evidence(id: &str, content: &str) -> IngestEvidence {
    IngestEvidence {
        status: Some(FactStatus::Rejected),
        ..evidence(id, content, &["Rejected"])
    }
}

fn code_hit() -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "main".to_owned(),
        resolved_commit_sha: "abc".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: Some("symbol:retry_policy".to_owned()),
        canonical_symbol_id: Some("repo://repo/src::lib::retry_policy".to_owned()),
        file_id: Some("file:src/lib.rs".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Impact],
        index_versions: vec!["code_graph:1".to_owned()],
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 1.0,
        excerpt: "fn retry_policy() {}".to_owned(),
    }
}

fn context(request_id: &str) -> RequestContext {
    RequestContext::with_ids(InterfaceKind::Cli, request_id, "trace-phase4-eval")
}
