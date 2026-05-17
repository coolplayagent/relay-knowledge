use std::collections::BTreeSet;

use crate::domain::{RerankDiagnostics, RerankMode, RerankSignal, RetrievalHit};

use super::{
    LOCAL_RERANK_MODEL, RerankConfig,
    terms::{extend_normalized_terms, normalized_terms},
};

const CONTENT_MATCH_WEIGHT: f64 = 0.40;
const ENTITY_MATCH_WEIGHT: f64 = 0.25;
const FACT_MATCH_WEIGHT: f64 = 0.20;
const PATH_MATCH_WEIGHT: f64 = 0.05;
const PER_EXTRA_SOURCE_BONUS: f64 = 0.05;
const GRAPH_FACT_BONUS: f64 = 0.08;
const SOURCE_SPAN_BONUS: f64 = 0.03;
const CODE_ARTIFACT_BONUS: f64 = 0.04;

pub(super) fn rerank_hits(
    query: &str,
    mut hits: Vec<RetrievalHit>,
    config: &RerankConfig,
) -> (Vec<RetrievalHit>, RerankDiagnostics) {
    let candidate_count = hits.len();
    if config.mode == RerankMode::Disabled {
        return (
            hits,
            RerankDiagnostics {
                requested_mode: RerankMode::Disabled,
                effective_mode: RerankMode::Disabled,
                algorithm: "reciprocal_rank_fusion_only".to_owned(),
                candidate_count,
                returned_count: candidate_count,
                degraded: false,
                reason: None,
            },
        );
    }

    let query_terms = terms_from_text(query);
    let model = match config.mode {
        RerankMode::Local => config.model.as_deref().unwrap_or(LOCAL_RERANK_MODEL),
        RerankMode::External => LOCAL_RERANK_MODEL,
        RerankMode::Disabled => unreachable!("disabled mode returns before scoring"),
    };
    for hit in &mut hits {
        let scored = score_hit(&query_terms, hit);
        hit.score = scored.score;
        hit.rerank = Some(RerankSignal {
            mode: RerankMode::Local,
            score: scored.score,
            explanation: format!(
                "local deterministic rerank model={model} rrf={:.4} content={:.2} entities={:.2} facts={:.2} path={:.2} sources={}",
                scored.rrf_score,
                scored.content_match,
                scored.entity_match,
                scored.fact_match,
                scored.path_match,
                hit.retriever_sources.len()
            ),
        });
    }
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.evidence_id.cmp(&right.evidence_id))
    });

    let degraded_reason = (config.mode == RerankMode::External).then(|| {
        "external rerank provider contract is reserved; using local deterministic rerank".to_owned()
    });
    (
        hits,
        RerankDiagnostics {
            requested_mode: config.mode,
            effective_mode: RerankMode::Local,
            algorithm: "deterministic_feature_rerank".to_owned(),
            candidate_count,
            returned_count: candidate_count,
            degraded: degraded_reason.is_some(),
            reason: degraded_reason,
        },
    )
}

#[derive(Debug, Clone, Copy)]
struct HitScore {
    score: f64,
    rrf_score: f64,
    content_match: f64,
    entity_match: f64,
    fact_match: f64,
    path_match: f64,
}

fn score_hit(query_terms: &BTreeSet<String>, hit: &RetrievalHit) -> HitScore {
    let rrf_score = hit.score;
    let content_match = term_coverage(query_terms, &terms_from_text(&hit.content));
    let entity_match = term_coverage(query_terms, &terms_from_labels(&hit.entity_labels));
    let fact_match = term_coverage(query_terms, &terms_from_facts(hit));
    let path_match = hit
        .source_path
        .as_deref()
        .map(|path| term_coverage(query_terms, &terms_from_text(path)))
        .unwrap_or(0.0);
    let score = rrf_score
        + content_match * CONTENT_MATCH_WEIGHT
        + entity_match * ENTITY_MATCH_WEIGHT
        + fact_match * FACT_MATCH_WEIGHT
        + path_match * PATH_MATCH_WEIGHT
        + source_diversity_bonus(hit)
        + evidence_structure_bonus(hit);

    HitScore {
        score,
        rrf_score,
        content_match,
        entity_match,
        fact_match,
        path_match,
    }
}

fn source_diversity_bonus(hit: &RetrievalHit) -> f64 {
    hit.retriever_sources.len().saturating_sub(1) as f64 * PER_EXTRA_SOURCE_BONUS
}

fn evidence_structure_bonus(hit: &RetrievalHit) -> f64 {
    let graph_fact_bonus = if hit.graph_facts.is_empty() {
        0.0
    } else {
        GRAPH_FACT_BONUS
    };
    let span_bonus = if hit.source_span.is_some() {
        SOURCE_SPAN_BONUS
    } else {
        0.0
    };
    let code_bonus = if hit.code_artifact.is_some() {
        CODE_ARTIFACT_BONUS
    } else {
        0.0
    };

    graph_fact_bonus + span_bonus + code_bonus
}

fn terms_from_facts(hit: &RetrievalHit) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for fact in &hit.graph_facts {
        extend_normalized_terms(&fact.subject, 1, &mut terms);
        extend_normalized_terms(&fact.predicate, 1, &mut terms);
        if let Some(object) = &fact.object {
            extend_normalized_terms(object, 1, &mut terms);
        }
    }

    terms
}

fn terms_from_labels(labels: &[String]) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for label in labels {
        extend_normalized_terms(label, 1, &mut terms);
    }

    terms
}

fn term_coverage(query_terms: &BTreeSet<String>, candidate_terms: &BTreeSet<String>) -> f64 {
    if query_terms.is_empty() || candidate_terms.is_empty() {
        return 0.0;
    }
    let matches = query_terms
        .iter()
        .filter(|term| candidate_terms.contains(*term))
        .count();

    matches as f64 / query_terms.len() as f64
}

fn terms_from_text(text: &str) -> BTreeSet<String> {
    normalized_terms(text, 1)
}

#[cfg(test)]
mod tests {
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

        let (reranked, diagnostics) =
            rerank_hits("SQLite async worker", hits, &RerankConfig::local());

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
}
