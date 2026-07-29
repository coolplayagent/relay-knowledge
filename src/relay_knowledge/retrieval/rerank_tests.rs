use crate::domain::{RerankMode, RetrieverSource};

use super::*;

#[test]
fn local_rerank_promotes_stronger_query_and_entity_match() {
    let hits = vec![
        hit(
            "ev-older",
            "generic runtime notes",
            &["Runtime"],
            vec![RetrieverSource::Bm25],
            0.12,
        ),
        hit(
            "ev-target",
            "SQLite worker isolates blocking writes from async query execution",
            &["SQLiteWorker"],
            vec![RetrieverSource::GraphEvidence, RetrieverSource::Semantic],
            0.10,
        ),
    ];

    let (reranked, diagnostics) = rerank_hits("SQLite async worker", hits, &RerankConfig::local());

    assert_eq!(reranked[0].evidence_id, "ev-target");
    assert_eq!(diagnostics.effective_mode, RerankMode::Local);
    assert!(reranked[0].rerank.is_some());
}

#[test]
fn local_rerank_matches_identifier_parts_in_entity_labels() {
    let hits = vec![
        hit(
            "ev-generic",
            "generic context pack note",
            &["Runtime"],
            vec![RetrieverSource::Bm25],
            0.12,
        ),
        hit(
            "ev-label",
            "opaque retrieval note",
            &["GraphRAGContextPack"],
            vec![RetrieverSource::Semantic, RetrieverSource::Vector],
            0.10,
        ),
    ];

    let (reranked, _) = rerank_hits("graph rag context pack", hits, &RerankConfig::local());

    assert_eq!(reranked[0].evidence_id, "ev-label");
}

#[test]
fn disabled_rerank_preserves_rrf_order_without_item_signal() {
    let config = RerankConfig {
        mode: RerankMode::Disabled,
        model: None,
        timeout: crate::retrieval::DEFAULT_RERANK_TIMEOUT,
        candidate_multiplier: 4,
        max_candidates: 64,
    };
    let hits = vec![
        hit("ev-a", "alpha", &[], vec![RetrieverSource::Bm25], 0.20),
        hit("ev-b", "beta", &[], vec![RetrieverSource::Bm25], 0.10),
    ];

    let (reranked, diagnostics) = rerank_hits("beta", hits, &config);

    assert_eq!(reranked[0].evidence_id, "ev-a");
    assert_eq!(diagnostics.effective_mode, RerankMode::Disabled);
    assert!(reranked.iter().all(|hit| hit.rerank.is_none()));
}

#[test]
fn external_rerank_explanation_reports_effective_local_model() {
    let config = RerankConfig {
        mode: RerankMode::External,
        model: Some("bge-reranker-v2".to_owned()),
        timeout: crate::retrieval::DEFAULT_RERANK_TIMEOUT,
        candidate_multiplier: 4,
        max_candidates: 64,
    };
    let hits = vec![hit(
        "ev-a",
        "rerank diagnostics",
        &[],
        vec![RetrieverSource::Bm25],
        0.20,
    )];

    let (reranked, diagnostics) = rerank_hits("rerank", hits, &config);
    let explanation = &reranked[0]
        .rerank
        .as_ref()
        .expect("external fallback should still attach a local rerank signal")
        .explanation;

    assert_eq!(diagnostics.requested_mode, RerankMode::External);
    assert_eq!(diagnostics.effective_mode, RerankMode::Local);
    assert!(explanation.contains(LOCAL_RERANK_MODEL));
    assert!(!explanation.contains("bge-reranker-v2"));
}

fn hit(
    evidence_id: &str,
    content: &str,
    labels: &[&str],
    sources: Vec<RetrieverSource>,
    score: f64,
) -> RetrievalHit {
    RetrievalHit {
        evidence_id: evidence_id.to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        source_span: None,
        content: content.to_owned(),
        entity_labels: labels.iter().map(|label| (*label).to_owned()).collect(),
        entities: Vec::new(),
        graph_facts: Vec::new(),
        code_artifact: None,
        retriever_sources: sources,
        ranking: Vec::new(),
        rerank: None,
        score,
    }
}
