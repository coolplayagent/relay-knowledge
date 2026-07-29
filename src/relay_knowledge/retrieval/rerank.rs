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
#[path = "rerank_tests.rs"]
mod tests;
