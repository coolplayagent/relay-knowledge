use std::sync::Arc;

use super::*;
use crate::{
    api::{IngestEvidence, InterfaceKind},
    application::RuntimeConfiguration,
    domain::{EvidenceRecord, FreshnessPolicy, GraphMutationBatch, SourceScope},
    env::{EnvironmentConfig, PlatformKind},
    storage::{GraphStore, KnowledgeStore, SqliteGraphStore},
};

#[tokio::test]
async fn retrieve_context_reports_truncated_context_pack_budget() {
    let service = service_with_memory_store().await;
    for index in 0..3 {
        service
            .ingest(
                IngestRequest {
                    source_scope: "docs".to_owned(),
                    evidence: vec![ingest_evidence(
                        format!("ev-{index}"),
                        format!("Shared BM25 retrieval candidate {index}"),
                        vec!["BM25".to_owned()],
                    )],
                    relations: Vec::new(),
                    claims: Vec::new(),
                    events: Vec::new(),
                },
                RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
            )
            .await
            .expect("ingest should succeed");
    }

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "BM25".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 2,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("query should succeed");

    assert!(response.truncated);
    assert!(response.context_pack.truncated);
    assert_eq!(response.results.len(), 2);
    assert_eq!(response.budget_used.limit, 2);
    assert_eq!(response.budget_used.returned_count, 2);
    assert_eq!(response.budget_used.candidate_count, 3);
    let trace = response
        .context_pack
        .provenance_trace
        .as_ref()
        .expect("context pack should include traversal trace");
    assert_eq!(trace.cited_evidence.len(), 2);
    assert!(!trace.visited_but_uncited.is_empty());
    assert!(trace.truncated);
}

#[tokio::test]
async fn retrieve_context_trace_marks_stale_indexes() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    let scope = SourceScope::parse("docs").expect("scope should parse");
    store
        .commit_mutation_batch(
            GraphMutationBatch::new(vec![
                EvidenceRecord::new(
                    "ev-stale",
                    scope,
                    "Stale indexes still return graph evidence context",
                    vec!["Stale".to_owned()],
                )
                .expect("evidence should validate"),
            ])
            .expect("batch should validate"),
        )
        .await
        .expect("direct commit should succeed");

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "stale evidence".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 5,
                freshness: FreshnessPolicy::AllowStale,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("query should succeed");

    let trace = response
        .context_pack
        .provenance_trace
        .as_ref()
        .expect("context pack should include traversal trace");
    assert!(trace.stale);
    assert!(
        trace
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("behind the graph version"))
    );
    assert!(
        trace
            .cited_evidence
            .iter()
            .any(|item| item.evidence_id == "ev-stale")
    );
}

#[tokio::test]
async fn retrieve_context_reports_trace_budget_truncation() {
    let service = service_with_memory_store().await;
    service
        .ingest(
            IngestRequest {
                source_scope: "docs".to_owned(),
                evidence: vec![ingest_evidence(
                    "ev-dense",
                    "Dense provenance context",
                    (0..20).map(|index| format!("Entity {index}")).collect(),
                )],
                relations: Vec::new(),
                claims: Vec::new(),
                events: Vec::new(),
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-ingest", "trace-ingest"),
        )
        .await
        .expect("ingest should succeed");

    let response = service
        .retrieve_context(
            HybridRetrievalRequest {
                query: "Dense provenance".to_owned(),
                source_scope: Some("docs".to_owned()),
                limit: 1,
                freshness: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::with_ids(InterfaceKind::Cli, "req-query", "trace-query"),
        )
        .await
        .expect("query should succeed");

    assert_eq!(response.results.len(), 1);
    assert!(response.truncated);
    assert!(response.context_pack.truncated);
    assert!(
        response
            .context_pack
            .provenance_trace
            .as_ref()
            .is_some_and(|trace| trace.truncated)
    );
}

async fn service_with_memory_store() -> RelayKnowledgeService {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    service_with_store(store).await
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
    RelayKnowledgeService::with_store(
        RuntimeConfiguration::from_environment(&environment)
            .await
            .expect("runtime"),
        store,
    )
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
