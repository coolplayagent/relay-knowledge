use super::*;
use crate::domain::{ContextEntity, RankingSignal};

#[test]
fn graph_search_outcome_applies_request_trace_budget() {
    let request = graph_search_request(1);
    let mut hit = retrieval_hit("ev-0", 1.0);
    hit.entities = (0..20)
        .map(|index| ContextEntity {
            id: format!("entity-{index}"),
            label: format!("Entity {index}"),
        })
        .collect();
    hit.ranking = (0..20)
        .map(|index| RankingSignal {
            source: RetrieverSource::GraphPath,
            rank: index + 1,
            score: 1.0 / (index + 1) as f64,
            explanation: format!("signal {index}"),
        })
        .collect();

    let outcome = GraphSearchOutcome::from_hits(&request, vec![hit]);
    let max_trace_items = request.max_trace_items();

    assert!(outcome.trace.truncated);
    assert!(outcome.trace.visited_nodes.len() <= max_trace_items);
    assert!(outcome.trace.ranking_contributions.len() <= max_trace_items);
}

#[test]
fn graph_search_trace_budget_preserves_requested_candidate_evidence() {
    let request = graph_search_request(80);
    let hits = (0..80)
        .map(|index| retrieval_hit(&format!("ev-{index:02}"), 100.0 - index as f64))
        .collect::<Vec<_>>();

    let outcome = GraphSearchOutcome::from_hits(&request, hits);

    assert_eq!(outcome.trace.visited_but_uncited.len(), 80);
    assert_eq!(outcome.trace.ranking_contributions.len(), 80);
    assert!(
        outcome
            .trace
            .visited_but_uncited
            .iter()
            .any(|evidence| evidence.evidence_id == "ev-79")
    );
}

fn graph_search_request(limit: usize) -> GraphSearchRequest {
    GraphSearchRequest {
        query: "trace".to_owned(),
        source_scope: Some("docs".to_owned()),
        graph_version: GraphVersion::new(1),
        limit,
        disabled_retriever_sources: Vec::new(),
    }
}

fn retrieval_hit(evidence_id: &str, score: f64) -> RetrievalHit {
    RetrievalHit {
        evidence_id: evidence_id.to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        source_span: None,
        content: format!("trace content {evidence_id}"),
        entity_labels: Vec::new(),
        entities: Vec::new(),
        graph_facts: Vec::new(),
        code_artifact: None,
        retriever_sources: vec![RetrieverSource::GraphPath],
        ranking: vec![RankingSignal {
            source: RetrieverSource::GraphPath,
            rank: 1,
            score,
            explanation: "graph path traversal".to_owned(),
        }],
        rerank: None,
        score,
    }
}
