use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::{
    code_query_api_identities,
    code_query_support::{query_is_single_symbol_identity, query_terms},
};

const API_CHUNK_FIRST_MIN_TERMS: usize = 4;
const STRUCTURED_SEQUENCE_MIN_TERMS: usize = 4;
const STRUCTURED_SEQUENCE_MIN_STRUCTURED_TERMS: usize = 3;
const PROCEDURAL_LANGUAGE_MIN_TERMS: usize = 6;
const PROCEDURAL_LANGUAGE_MIN_HIGH_SIGNAL_TERMS: usize = 5;
const HIGH_SIGNAL_TERM_LEN: usize = 5;

pub(super) fn hybrid_query_prefers_chunk_first(request: &CodeRetrievalRequest) -> bool {
    if request.code_query_kind != CodeQueryKind::Hybrid
        || query_is_single_symbol_identity(&request.query)
    {
        return false;
    }

    let raw_terms = query_terms(&request.query);
    let terms = hybrid_sequence_terms_from_raw(raw_terms.iter().map(String::as_str));
    if terms.len() < API_CHUNK_FIRST_MIN_TERMS {
        return false;
    }
    if !code_query_api_identities::hybrid_api_symbol_identities(&request.query, request).is_empty()
    {
        return true;
    }

    hybrid_query_has_structured_sequence(&raw_terms, &terms)
        || hybrid_query_has_filtered_procedural_surface(request, &terms)
}

pub(super) fn hybrid_sequence_terms(query: &str) -> Vec<String> {
    let raw_terms = query_terms(query);
    hybrid_sequence_terms_from_raw(raw_terms.iter().map(String::as_str))
}

fn hybrid_sequence_terms_from_raw<'a>(raw_terms: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut terms = Vec::new();
    for term in raw_terms {
        if term.len() < API_CHUNK_FIRST_MIN_TERMS {
            continue;
        }
        let term = term.to_ascii_lowercase();
        if !terms.contains(&term) {
            terms.push(term);
        }
    }

    terms
}

fn hybrid_query_has_structured_sequence(raw_terms: &[String], terms: &[String]) -> bool {
    terms.len() >= STRUCTURED_SEQUENCE_MIN_TERMS
        && raw_terms
            .iter()
            .filter(|term| term.len() >= API_CHUNK_FIRST_MIN_TERMS)
            .filter(|term| hybrid_term_has_structure(term))
            .count()
            >= STRUCTURED_SEQUENCE_MIN_STRUCTURED_TERMS
}

fn hybrid_query_has_filtered_procedural_surface(
    request: &CodeRetrievalRequest,
    terms: &[String],
) -> bool {
    request
        .repository
        .language_filters
        .iter()
        .any(|language| procedural_chunk_first_language(language))
        && terms.len() >= PROCEDURAL_LANGUAGE_MIN_TERMS
        && terms
            .iter()
            .filter(|term| hybrid_sequence_term_has_high_signal(term))
            .count()
            >= PROCEDURAL_LANGUAGE_MIN_HIGH_SIGNAL_TERMS
}

fn hybrid_sequence_term_has_high_signal(term: &str) -> bool {
    term.chars().count() >= HIGH_SIGNAL_TERM_LEN
        || term.contains('_')
        || term_has_alpha_digit_mix(term)
}

fn hybrid_term_has_structure(term: &str) -> bool {
    term.contains('_') || term_has_alpha_digit_mix(term) || term_has_case_boundary(term)
}

fn procedural_chunk_first_language(language: &str) -> bool {
    matches!(
        language.to_ascii_lowercase().as_str(),
        "c" | "cc" | "cpp" | "c++" | "cxx" | "h" | "hh" | "hpp" | "hxx"
    )
}

fn term_has_case_boundary(term: &str) -> bool {
    let mut previous = None;
    term.chars().any(|character| {
        let boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|previous: char| previous.is_ascii_lowercase());
        previous = Some(character);
        boundary
    })
}

fn term_has_alpha_digit_mix(term: &str) -> bool {
    let mut has_alpha = false;
    let mut has_digit = false;
    for character in term.chars() {
        has_alpha |= character.is_ascii_alphabetic();
        has_digit |= character.is_ascii_digit();
    }

    has_alpha && has_digit
}
