use crate::domain::{CodeQueryKind, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest};

use super::{
    code_query_api_identities::{ApiSymbolIdentity, hybrid_api_symbol_identities},
    code_query_hybrid_planning::{
        hybrid_query_has_conversion_expansion_intent,
        hybrid_query_has_declaration_expansion_intent, hybrid_query_has_inline_expansion_intent,
        hybrid_sequence_terms,
    },
};

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
    if hybrid_query_has_declaration_expansion_intent(&request.query) {
        return false;
    }
    if hybrid_query_has_conversion_expansion_intent(&request.query) {
        return false;
    }
    if hybrid_query_has_inline_expansion_intent(&request.query) {
        return false;
    }

    if hybrid_pascal_identifier_hit_covers_query(request, hits) {
        return true;
    }
    if terms.len() <= 4
        && hits
            .iter()
            .take(request.limit.max(1))
            .any(|hit| hybrid_direct_hit_covers_query(hit, &terms))
    {
        return true;
    }
    if hybrid_direct_lexical_surface_covers_query(request, hits, &terms) {
        return true;
    }

    hybrid_api_identity_symbol_hits_cover_query(request, hits)
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

fn hybrid_direct_lexical_surface_covers_query(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
    terms: &[String],
) -> bool {
    if terms.len() < 5 {
        return false;
    }
    let required_coverage = terms.len().saturating_mul(2).div_ceil(3).max(4);
    let required_supporting_hits = 3;
    let mut covered_terms = Vec::new();
    let mut supporting_hits = 0usize;
    for hit in hits.iter().take(request.limit.max(1)) {
        if !hybrid_direct_hit_can_answer(hit) {
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
        if matched_terms >= 2 && hit.score >= 4.0 {
            supporting_hits += 1;
        }
    }

    supporting_hits >= required_supporting_hits && covered_terms.len() >= required_coverage
}

fn hybrid_pascal_identifier_hit_covers_query(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    let identifiers = pascal_identifier_terms(&request.query);
    if identifiers.is_empty() {
        return false;
    }

    hits.iter()
        .take(request.limit.max(1))
        .filter(|hit| hybrid_direct_hit_can_answer(hit) && hit.score >= 4.0)
        .any(|hit| {
            identifiers
                .iter()
                .any(|identifier| hit.excerpt.contains(identifier))
        })
}

fn pascal_identifier_terms(query: &str) -> Vec<String> {
    let mut identifiers = Vec::new();
    for token in query.split_whitespace().map(|term| {
        term.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric() || character == '_')
        })
    }) {
        if token.len() < 6 || token.contains('_') {
            continue;
        }
        let Some(first) = token.chars().next() else {
            continue;
        };
        if !first.is_ascii_uppercase() || identifier_case_boundary_count(token) < 2 {
            continue;
        }
        if !identifiers
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(token))
        {
            identifiers.push(token.to_owned());
        }
    }

    identifiers
}

fn identifier_case_boundary_count(term: &str) -> usize {
    let mut boundaries = 0usize;
    let mut previous_lowercase = false;
    for character in term.chars() {
        if character.is_ascii_uppercase() && previous_lowercase {
            boundaries += 1;
        }
        previous_lowercase = character.is_ascii_lowercase() || character.is_ascii_digit();
    }

    boundaries
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

fn hybrid_api_identity_symbol_hits_cover_query(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    let identities = hybrid_api_symbol_identities(&request.query, request);
    if identities.len() < 2 || identities.len() > request.limit.max(1) {
        return false;
    }

    identities.iter().all(|identity| {
        hits.iter()
            .any(|hit| api_identity_symbol_hit_matches(hit, identity))
    })
}

fn api_identity_symbol_hit_matches(hit: &CodeRetrievalHit, identity: &ApiSymbolIdentity) -> bool {
    if !hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
        || !hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::Definition)
        || hit
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
        || hit.edge_kind.is_some()
    {
        return false;
    }
    let Some(canonical_symbol_id) = hit.canonical_symbol_id.as_deref() else {
        return false;
    };
    let Some(leaf_name) = canonical_symbol_leaf(canonical_symbol_id) else {
        return false;
    };

    identity.matches_symbol(leaf_name, &hit.excerpt, &hit.excerpt, canonical_symbol_id)
}

fn canonical_symbol_leaf(canonical_symbol_id: &str) -> Option<&str> {
    canonical_symbol_id
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
}

#[cfg(test)]
#[path = "direct_gate_tests.rs"]
mod tests;
