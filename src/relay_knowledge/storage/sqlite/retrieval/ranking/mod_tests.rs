//! Direct reciprocal-rank fusion and duplicate-merge invariants.

use super::*;

fn hit(evidence_id: &str, content: &str) -> RetrievalHit {
    RetrievalHit {
        evidence_id: evidence_id.to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        source_span: None,
        content: content.to_owned(),
        entity_labels: Vec::new(),
        entities: Vec::new(),
        graph_facts: Vec::new(),
        code_artifact: None,
        retriever_sources: Vec::new(),
        ranking: Vec::new(),
        rerank: None,
        score: 0.0,
    }
}

#[test]
fn merge_ranked_fuses_duplicate_evidence_without_duplicate_sources() {
    let mut candidates = BTreeMap::new();
    let hits = vec![
        ScoredHit {
            key: "evidence:ev-1".to_owned(),
            hit: hit("ev-1", "first"),
            source: RetrieverSource::Bm25,
            source_score: 0.8,
            modality: "text".to_owned(),
            explanation: None,
        },
        ScoredHit {
            key: "evidence:ev-1".to_owned(),
            hit: hit("ev-1", "second"),
            source: RetrieverSource::Bm25,
            source_score: 0.6,
            modality: "text".to_owned(),
            explanation: Some("secondary match".to_owned()),
        },
    ];

    merge_ranked(
        &mut candidates,
        hits,
        RetrieverSource::Semantic,
        "semantic match",
    );
    let fused = candidates
        .remove("evidence:ev-1")
        .expect("candidate should exist")
        .into_hit();

    assert_eq!(fused.content, "first\n\nsecond");
    assert_eq!(fused.retriever_sources, [RetrieverSource::Semantic]);
    assert_eq!(fused.ranking.len(), 2);
    assert!(fused.score > 0.0);
    assert_eq!(
        fused.ranking[0].explanation,
        "semantic match; modality=text"
    );
    assert_eq!(fused.ranking[1].explanation, "secondary match");
}
