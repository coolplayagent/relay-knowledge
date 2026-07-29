use crate::domain::{CodeQueryKind, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest};

use super::{
    code_query_hybrid_direct_gate::hybrid_direct_results_can_answer_without_graph_expansion,
    code_query_hybrid_planning::{
        hybrid_query_has_conversion_expansion_intent,
        hybrid_query_has_declaration_expansion_intent, hybrid_query_has_inline_expansion_intent,
        hybrid_sequence_terms, query_language_scoped_workflow_surface_scopes,
        workflow_language_scope_matches,
    },
};

pub(super) fn hybrid_chunk_results_can_answer_without_graph_expansion(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Hybrid {
        return false;
    }
    if hybrid_query_has_declaration_expansion_intent(&request.query) {
        return false;
    }
    if hybrid_query_has_conversion_expansion_intent(&request.query) {
        return false;
    }
    if hybrid_query_has_inline_expansion_intent(&request.query) {
        return false;
    }
    let terms = hybrid_sequence_terms(&request.query);
    if terms.len() < 3 {
        return false;
    }
    let language_scopes = query_language_scoped_workflow_surface_scopes(request);
    let required_matches = terms.len().clamp(3, 4);
    let required_hits = request.limit.clamp(1, 3);
    let dense_chunk_hits = hits
        .iter()
        .filter(|hit| {
            hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
                && !hit
                    .retrieval_layers
                    .contains(&CodeRetrievalLayer::TextFallback)
                && workflow_language_scopes_allow_hit(&language_scopes, &hit.language_id)
                && hybrid_sequence_match_count(&hit.excerpt, &terms) >= required_matches
        })
        .take(required_hits)
        .count();
    if dense_chunk_hits >= required_hits {
        return true;
    }

    hybrid_chunk_results_have_collective_dense_coverage(
        &terms,
        hits,
        required_hits,
        &language_scopes,
    )
}

pub(super) fn hybrid_hits_can_answer_without_graph_expansion(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    hybrid_chunk_results_can_answer_without_graph_expansion(request, hits)
        || hybrid_direct_results_can_answer_without_graph_expansion(request, hits)
}

fn hybrid_chunk_results_have_collective_dense_coverage(
    terms: &[String],
    hits: &[CodeRetrievalHit],
    required_hits: usize,
    language_scopes: &[&str],
) -> bool {
    let required_coverage = terms.len().saturating_mul(2).div_ceil(3).max(4);
    let required_dense_matches = terms.len().clamp(3, 4);
    let mut covered_terms = Vec::new();
    let mut supporting_hits = 0usize;
    let mut has_dense_hit = false;
    for hit in hits {
        if !hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
            || hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::TextFallback)
            || !workflow_language_scopes_allow_hit(language_scopes, &hit.language_id)
        {
            continue;
        }
        let excerpt = hit.excerpt.to_ascii_lowercase();
        let mut matched_terms = 0usize;
        for term in terms {
            if excerpt.contains(term.as_str()) {
                matched_terms += 1;
                if !covered_terms.contains(term) {
                    covered_terms.push(term.clone());
                }
            }
        }
        if matched_terms >= 2 {
            supporting_hits += 1;
        }
        has_dense_hit |= matched_terms >= required_dense_matches;
    }

    supporting_hits >= required_hits && has_dense_hit && covered_terms.len() >= required_coverage
}

fn workflow_language_scopes_allow_hit(language_scopes: &[&str], language_id: &str) -> bool {
    language_scopes.is_empty()
        || language_scopes
            .iter()
            .any(|scope| workflow_language_scope_matches(language_id, scope))
}

pub(super) fn retain_query_language_scoped_workflow_hits(
    request: &CodeRetrievalRequest,
    hits: &mut Vec<CodeRetrievalHit>,
) {
    let language_scopes = query_language_scoped_workflow_surface_scopes(request);
    if language_scopes.is_empty() {
        return;
    }

    hits.retain(|hit| workflow_language_scopes_allow_hit(&language_scopes, &hit.language_id));
}

fn hybrid_sequence_match_count(excerpt: &str, terms: &[String]) -> usize {
    let excerpt = excerpt.to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| excerpt.contains(term.as_str()))
        .count()
}

#[cfg(test)]
#[path = "chunk_gate_tests.rs"]
mod tests;
