use crate::domain::{CodeQueryKind, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest};

use super::code_query_hybrid_planning::hybrid_sequence_terms;

pub(super) fn hybrid_direct_results_can_answer_without_graph_expansion(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Hybrid {
        return false;
    }
    let terms = hybrid_sequence_terms(&request.query);
    if terms.len() < 3 {
        return false;
    }
    if hybrid_query_has_graph_expansion_intent(&terms) {
        return false;
    }

    hits.iter()
        .take(request.limit.max(1))
        .any(|hit| hybrid_direct_hit_covers_query(hit, &terms))
}

fn hybrid_direct_hit_covers_query(hit: &CodeRetrievalHit, terms: &[String]) -> bool {
    hybrid_direct_hit_can_answer(hit)
        && hybrid_sequence_match_count(&hit.excerpt, terms)
            >= hybrid_direct_required_match_count(terms.len())
}

fn hybrid_direct_required_match_count(term_count: usize) -> usize {
    term_count
        .saturating_mul(4)
        .div_ceil(5)
        .clamp(4, 6)
        .min(term_count)
}

fn hybrid_query_has_graph_expansion_intent(terms: &[String]) -> bool {
    terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "caller"
                | "callers"
                | "callee"
                | "callees"
                | "reference"
                | "references"
                | "referenced"
                | "import"
                | "imports"
                | "importer"
                | "importers"
        )
    })
}

fn hybrid_direct_hit_can_answer(hit: &CodeRetrievalHit) -> bool {
    if hit.edge_kind.is_some()
        || hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    {
        return false;
    }

    hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
        || (hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::Definition))
}

fn hybrid_sequence_match_count(excerpt: &str, terms: &[String]) -> usize {
    let excerpt = excerpt.to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| excerpt.contains(term.as_str()))
        .count()
}
