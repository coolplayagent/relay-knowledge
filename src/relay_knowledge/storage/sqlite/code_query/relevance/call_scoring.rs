//! Scores call edges using direction, confidence, repetition, and endpoint identity.

use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::{text_scoring::ScoreQuery, tokens::identifier_search_tokens};

pub(in crate::storage::sqlite::code::code_query) fn call_edge_confidence_bonus(
    confidence_basis_points: u16,
) -> f64 {
    f64::from(confidence_basis_points) / 10_000.0
}

pub(in crate::storage::sqlite::code::code_query) fn repeated_call_site_bonus(
    base_score: f64,
    call_site_count: usize,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Callers
        || call_site_count <= 1
    {
        return 0.0;
    }

    (call_site_count.saturating_sub(1).min(3) as f64) * 0.25
}

pub(in crate::storage::sqlite::code::code_query) fn callee_related_name_bonus(
    query: &str,
    callee_name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Callees {
        return 0.0;
    }
    let query_tokens = identifier_search_tokens(query);
    if query_tokens.is_empty() {
        return 0.0;
    }
    let callee_tokens = identifier_search_tokens(callee_name);
    if query_tokens.iter().any(|query_token| {
        query_token.len() > 2
            && callee_tokens
                .iter()
                .any(|callee_token| callee_token == query_token)
    }) {
        if callee_name_is_query_fragment(&query_tokens, &callee_tokens) {
            0.15
        } else {
            0.35 + (1.2 / callee_identifier_part_count(callee_name))
        }
    } else {
        0.0
    }
}

fn callee_name_is_query_fragment(query_tokens: &[String], callee_tokens: &[String]) -> bool {
    !callee_tokens.is_empty()
        && query_tokens.len() > callee_tokens.len()
        && callee_tokens
            .iter()
            .all(|callee| query_tokens.iter().any(|query| query == callee))
}

pub(in crate::storage::sqlite::code::code_query) fn directional_call_context_bonus(
    query: &ScoreQuery,
    base_score: f64,
    caller_name: Option<&str>,
    callee_name: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 {
        return 0.0;
    }
    match request.code_query_kind {
        CodeQueryKind::Callers => 0.35 * query.score([caller_name.unwrap_or_default(), path]),
        CodeQueryKind::Callees => 0.35 * query.score([callee_name, path]),
        _ => 0.0,
    }
}

pub(in crate::storage::sqlite::code::code_query) fn same_named_caller_penalty(
    caller_name: Option<&str>,
    callee_name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::Callers {
        return 0.0;
    }
    let Some(caller_leaf) = caller_name.and_then(leaf_identifier) else {
        return 0.0;
    };
    let Some(callee_leaf) = leaf_identifier(callee_name) else {
        return 0.0;
    };
    let caller = compact_identifier(&caller_leaf);
    let callee = compact_identifier(&callee_leaf);
    if !caller.is_empty() && caller == callee {
        -2.5
    } else {
        0.0
    }
}

fn leaf_identifier(value: &str) -> Option<String> {
    value
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|term| !term.is_empty())
        .map(str::to_owned)
}

fn compact_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn callee_identifier_part_count(callee_name: &str) -> f64 {
    let part_count = identifier_tokens(callee_name)
        .flat_map(|token| token.split('_'))
        .filter(|part| !part.is_empty())
        .count()
        .max(1);

    part_count as f64
}

fn identifier_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
}
