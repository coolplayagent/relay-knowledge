//! Public FTS match planning over focused, hybrid, lifecycle, and structured recall.

#[cfg(test)]
use super::{
    fts_compound::MAX_COMPOUND_FTS_ALTERNATIVES, fts_recall::MAX_HYBRID_CHUNK_RECALL_TERMS,
};
use super::{
    fts_compound::{
        compound_identifier_fts_terms, compound_identifier_source_term,
        push_compound_identifier_window,
    },
    fts_recall::{
        api_dense_hybrid_query, hybrid_chunk_recall_terms, strict_hybrid_chunk_recall_terms,
        strict_member_access_recall_allowed,
    },
    fts_terms::{
        MAX_HYBRID_CHUNK_RECALL_ANCHORS, MIN_HIGH_SIGNAL_TERM_PRIORITY,
        append_type_surface_companion_terms, dedupe_terms, hybrid_chunk_term_priority,
        identifier_term_has_recall_structure, identifier_term_has_structure,
        push_case_insensitive_unique_term, quote_fts_term,
    },
};

const EMPTY_FTS_QUERY: &str = "relayknowledgeunlikelyemptyquerytoken";
const MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS: usize = 4;
const STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS: usize = 2;
const FOCUSED_HYBRID_CHUNK_MAX_TERMS: usize = 8;
const FOCUSED_HYBRID_CHUNK_PAIR_DISTANCE: usize = 4;
const COMPOUND_HYBRID_CHUNK_MIN_TERM_LEN: usize = 4;
const COMPOUND_HYBRID_CHUNK_MAX_TERMS: usize = 8;
const COMPOUND_HYBRID_CHUNK_PAIR_DISTANCE: usize = 1;
const FOCUSED_SYMBOL_MAX_TERMS: usize = 3;
const FOCUSED_SYMBOL_MAX_WORKFLOW_TERMS: usize = 2;
const FOCUSED_SYMBOL_MORPHOLOGY_MIN_PREFIX_LEN: usize = 6;
const FOCUSED_SYMBOL_MORPHOLOGY_TERM_COUNT: usize = 3;

pub(in crate::storage::sqlite::code::query) fn fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(&super::candidate_plan::fts_query_terms(query), " ", true)
}

pub(in crate::storage::sqlite::code::query) fn symbol_fts_match_query(query: &str) -> String {
    fts_match_query_with_operator(&super::candidate_plan::fts_query_terms(query), " OR ", true)
}

pub(in crate::storage::sqlite::code::query) fn focused_symbol_fts_match_query(
    query: &str,
) -> Option<String> {
    focused_symbol_recall_terms(query)
        .map(|terms| fts_match_query_with_operator(&terms, " OR ", false))
}

pub(in crate::storage::sqlite::code::query) fn focused_symbol_morphology_fts_match_query(
    query: &str,
) -> Option<String> {
    let prefix_terms = focused_symbol_recall_terms(query)?
        .into_iter()
        .filter(|term| {
            term.chars().count() >= FOCUSED_SYMBOL_MORPHOLOGY_MIN_PREFIX_LEN
                && term
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
        })
        .take(FOCUSED_SYMBOL_MORPHOLOGY_TERM_COUNT)
        .collect::<Vec<_>>();
    if prefix_terms.len() < FOCUSED_SYMBOL_MORPHOLOGY_TERM_COUNT {
        return None;
    }

    let mut pairs = Vec::with_capacity(FOCUSED_SYMBOL_MORPHOLOGY_TERM_COUNT);
    for (index, left) in prefix_terms.iter().enumerate() {
        for right in prefix_terms.iter().skip(index + 1) {
            pairs.push(format!(
                "({}* {}*)",
                quote_fts_term(left),
                quote_fts_term(right)
            ));
        }
    }

    Some(pairs.join(" OR "))
}

fn focused_symbol_recall_terms(query: &str) -> Option<Vec<String>> {
    let terms = dedupe_terms(super::candidate_plan::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return None;
    }
    let mut ranked = terms
        .iter()
        .enumerate()
        .filter(|(_, term)| !focused_symbol_generic_term(term))
        .map(|(position, term)| {
            (
                identifier_term_has_structure(term),
                hybrid_chunk_term_priority(term),
                position,
                term,
            )
        })
        .filter(|(_, priority, _, _)| *priority >= 2)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(right.3))
    });
    let mut recall_terms = ranked
        .into_iter()
        .map(|(_, _, _, term)| term.to_owned())
        .take(FOCUSED_SYMBOL_MAX_TERMS)
        .collect::<Vec<_>>();
    append_focused_symbol_workflow_terms(&terms, &mut recall_terms);
    append_type_surface_companion_terms(&terms, &mut recall_terms);

    (recall_terms.len() >= 2).then_some(recall_terms)
}

fn append_focused_symbol_workflow_terms(terms: &[String], recall_terms: &mut Vec<String>) {
    let mut appended = 0usize;
    for term in terms {
        if appended >= FOCUSED_SYMBOL_MAX_WORKFLOW_TERMS {
            break;
        }
        if focused_symbol_workflow_term(term) {
            let before = recall_terms.len();
            push_case_insensitive_unique_term(recall_terms, term);
            if recall_terms.len() > before {
                appended += 1;
            }
        }
    }
}

fn focused_symbol_workflow_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "connect" | "connection" | "event" | "run" | "source" | "stream"
    )
}

fn focused_symbol_generic_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "call"
            | "arrow"
            | "client"
            | "contract"
            | "flow"
            | "function"
            | "generic"
            | "handler"
            | "interface"
            | "literal"
            | "object"
            | "provider"
            | "record"
            | "request"
            | "response"
            | "service"
            | "typed"
            | "type"
    )
}

pub(in crate::storage::sqlite::code::query) fn hybrid_chunk_fts_match_query(query: &str) -> String {
    hybrid_chunk_fts_match_query_with_compound(query, true)
}

pub(in crate::storage::sqlite::code::query) fn direct_hybrid_chunk_fts_match_query(
    query: &str,
) -> String {
    hybrid_chunk_fts_match_query_with_compound(query, false)
}

pub(in crate::storage::sqlite::code::query) fn focused_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(super::candidate_plan::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return None;
    }
    if terms.iter().any(|term| identifier_term_has_structure(term)) {
        return None;
    }
    let terms = terms
        .into_iter()
        .filter(|term| term.len() >= MIN_HIGH_SIGNAL_TERM_PRIORITY)
        .take(FOCUSED_HYBRID_CHUNK_MAX_TERMS)
        .collect::<Vec<_>>();
    if terms.len() < 3 {
        return None;
    }
    let mut groups = Vec::new();
    for (index, left) in terms.iter().enumerate() {
        for right in terms
            .iter()
            .skip(index + 1)
            .take(FOCUSED_HYBRID_CHUNK_PAIR_DISTANCE)
        {
            groups.push(format!(
                "({} {})",
                quote_fts_term(left),
                quote_fts_term(right)
            ));
        }
    }

    (!groups.is_empty()).then(|| groups.join(" OR "))
}

pub(in crate::storage::sqlite::code::query) fn lifecycle_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(
        super::candidate_plan::fts_query_terms(query)
            .into_iter()
            .map(|term| term.to_ascii_lowercase())
            .collect(),
    );
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return None;
    }
    let finalization_terms = lifecycle_finalization_recall_terms(&terms);
    if finalization_terms.is_empty() {
        return None;
    }
    let has_tool_call_intent = terms
        .iter()
        .any(|term| matches!(term.as_str(), "tool" | "tools"))
        && terms
            .iter()
            .any(|term| matches!(term.as_str(), "call" | "calls"));
    let has_lifecycle_intent = terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "delta" | "event" | "events" | "lifecycle" | "stream"
        )
    });
    if !has_tool_call_intent || !has_lifecycle_intent {
        return None;
    }

    let anchor = if terms.iter().any(|term| term == "delta") {
        "delta"
    } else if terms.iter().any(|term| term == "tool") {
        "tool"
    } else {
        "lifecycle"
    };
    Some(lifecycle_recall_match_query(anchor, &finalization_terms))
}

fn lifecycle_finalization_recall_terms(terms: &[String]) -> Vec<String> {
    let mut recall_terms = Vec::new();
    for term in terms {
        match term.as_str() {
            "finish" | "finalize" | "finalized" => recall_terms.push(term.clone()),
            "finished" => {
                recall_terms.push("finish".to_owned());
                recall_terms.push("finished".to_owned());
            }
            _ => {}
        }
    }
    recall_terms.sort();
    recall_terms.dedup();

    recall_terms
}

fn lifecycle_recall_match_query(anchor: &str, finalization_terms: &[String]) -> String {
    finalization_terms
        .iter()
        .map(|term| format!("{} {}", quote_fts_term(anchor), quote_fts_term(term)))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(in crate::storage::sqlite::code::query) fn structured_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let query_terms = dedupe_terms(super::candidate_plan::fts_query_terms(query));
    let mut terms = query_terms
        .into_iter()
        .filter(|term| identifier_term_has_recall_structure(term))
        .take(MAX_HYBRID_CHUNK_RECALL_ANCHORS)
        .collect::<Vec<_>>();
    append_type_surface_companion_terms(
        &dedupe_terms(super::candidate_plan::fts_query_terms(query)),
        &mut terms,
    );

    (!terms.is_empty()).then(|| fts_match_query_with_operator(&terms, " OR ", false))
}

pub(in crate::storage::sqlite::code::query) fn compound_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(super::candidate_plan::fts_query_terms(query))
        .into_iter()
        .filter(|term| term.len() >= COMPOUND_HYBRID_CHUNK_MIN_TERM_LEN)
        .take(COMPOUND_HYBRID_CHUNK_MAX_TERMS)
        .collect::<Vec<_>>();
    if terms.len() < 2 {
        return None;
    }

    let mut alternatives = Vec::new();
    for (index, left) in terms.iter().enumerate() {
        for right in terms
            .iter()
            .skip(index + 1)
            .take(COMPOUND_HYBRID_CHUNK_PAIR_DISTANCE)
        {
            push_compound_identifier_window(
                &mut alternatives,
                &terms,
                &[left.clone(), right.clone()],
            );
        }
    }

    (!alternatives.is_empty()).then(|| {
        alternatives
            .iter()
            .map(|term| quote_fts_term(term))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

fn hybrid_chunk_fts_match_query_with_compound(
    query: &str,
    include_compound_identifiers: bool,
) -> String {
    let mut terms = dedupe_terms(super::candidate_plan::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        let query_terms = terms.clone();
        append_type_surface_companion_terms(&query_terms, &mut terms);
        return fts_match_query_with_operator(&terms, " OR ", include_compound_identifiers);
    }

    let recall_terms = hybrid_chunk_recall_terms(&terms);
    fts_match_query_with_operator(&recall_terms, " OR ", include_compound_identifiers)
}

pub(in crate::storage::sqlite::code::query) fn strict_hybrid_chunk_fts_match_query(
    query: &str,
) -> Option<String> {
    let terms = dedupe_terms(super::candidate_plan::fts_query_terms(query));
    if terms.len() <= MAX_HYBRID_CHUNK_SIMPLE_RECALL_TERMS {
        return None;
    }
    let strict_terms = strict_hybrid_chunk_recall_terms(query, &terms);
    if !api_dense_hybrid_query(&terms) && !strict_member_access_recall_allowed(query, &strict_terms)
    {
        return None;
    }
    (strict_terms.len() >= STRICT_HYBRID_CHUNK_MIN_STRUCTURED_TERMS)
        .then(|| fts_match_query_with_operator(&strict_terms, " ", false))
}

fn fts_match_query_with_operator(
    terms: &[String],
    operator: &str,
    include_compound_identifiers: bool,
) -> String {
    if terms.is_empty() {
        return EMPTY_FTS_QUERY.to_owned();
    }

    let primary = terms
        .iter()
        .map(|term| quote_fts_term(term))
        .collect::<Vec<_>>()
        .join(operator);
    let alternatives = if include_compound_identifiers {
        let compound_terms = terms
            .iter()
            .filter(|term| compound_identifier_source_term(term))
            .cloned()
            .collect::<Vec<_>>();
        compound_identifier_fts_terms(&compound_terms)
    } else {
        Vec::new()
    };
    if alternatives.is_empty() {
        primary
    } else {
        format!(
            "({}) OR {}",
            primary,
            alternatives
                .iter()
                .map(|term| quote_fts_term(term))
                .collect::<Vec<_>>()
                .join(" OR ")
        )
    }
}

#[cfg(test)]
#[path = "fts_plan_tests.rs"]
mod tests;
