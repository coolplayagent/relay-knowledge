use super::super::{
    code_query_conversion_terms::conversion_action_term,
    code_query_identifiers::identifier_terms_equivalent,
};
use super::identifier_search_tokens;
use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

pub(super) fn conversion_symbol_bonus(
    query: &str,
    name: &str,
    signature: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Hybrid {
        return 0.0;
    }
    let query_terms = identifier_search_tokens(query);
    if !query_terms.iter().any(|term| conversion_action_term(term)) {
        return 0.0;
    }
    let query_surface_terms = query_terms
        .iter()
        .filter(|term| conversion_surface_query_term(term))
        .cloned()
        .collect::<Vec<_>>();
    if query_surface_terms.len() < 2 {
        return 0.0;
    }

    let name_terms = identifier_search_tokens(name);
    if !name_terms
        .iter()
        .any(|term| conversion_symbol_name_action_term(term))
    {
        return 0.0;
    }
    let signature_terms = identifier_search_tokens(signature);
    let signature_matches = matching_query_surface_count(&query_surface_terms, &signature_terms);
    let name_matches = matching_query_surface_count(&query_surface_terms, &name_terms);
    if signature_matches == 0 || signature_matches + name_matches < 2 {
        return 0.0;
    }

    if name_terms
        .iter()
        .any(|term| matches!(term.as_str(), "from" | "to"))
        && signature_return_matches_query_surface(signature, &query_surface_terms)
    {
        3.6
    } else {
        2.0
    }
}

fn conversion_symbol_name_action_term(term: &str) -> bool {
    conversion_action_term(term) || matches!(term, "from" | "to")
}

fn conversion_surface_query_term(term: &str) -> bool {
    term.len() >= 4
        && !conversion_action_term(term)
        && !matches!(
            term,
            "call"
                | "calls"
                | "from"
                | "into"
                | "that"
                | "this"
                | "tool"
                | "tools"
                | "type"
                | "types"
                | "with"
        )
}

fn matching_query_surface_count(query_surface_terms: &[String], symbol_terms: &[String]) -> usize {
    query_surface_terms
        .iter()
        .filter(|query_term| {
            symbol_terms
                .iter()
                .any(|symbol_term| identifier_terms_equivalent(symbol_term, query_term))
        })
        .count()
}

fn signature_return_matches_query_surface(signature: &str, query_surface_terms: &[String]) -> bool {
    let Some(return_start) = signature
        .rfind("):")
        .map(|index| index + 2)
        .or_else(|| signature.rfind("->").map(|index| index + 2))
    else {
        return false;
    };
    let return_terms = identifier_search_tokens(&signature[return_start..]);

    matching_query_surface_count(query_surface_terms, &return_terms) > 0
}
