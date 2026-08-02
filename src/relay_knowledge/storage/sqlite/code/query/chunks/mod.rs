use crate::domain::{CodeQueryKind, CodeRetrievalHit, CodeRetrievalRequest};

use super::{references::reference_usage_context_bonus, relevance::SymbolIdentityQuery};

mod search;
pub(super) use search::search_chunks;

pub(super) fn definition_query_needs_chunk_fallback(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Definition {
        return false;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return hits.is_empty();
    };

    !hits.iter().any(|hit| {
        hit.canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol_id| canonical_symbol_leaf_matches(symbol_id, identity.leaf_name()))
    })
}

pub(super) fn references_query_needs_chunk_fallback(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    request.code_query_kind == CodeQueryKind::References
        && hits.is_empty()
        && SymbolIdentityQuery::from_query(&request.query).is_some()
}

pub(super) fn canonical_symbol_leaf_matches(canonical_symbol_id: &str, leaf_name: &str) -> bool {
    canonical_symbol_id
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
        .is_some_and(|part| part == leaf_name)
}

pub(super) fn exact_reference_chunk_bonus(
    request: &CodeRetrievalRequest,
    base_score: f64,
    content: &str,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::References {
        return 0.0;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return 0.0;
    };

    reference_usage_context_bonus(
        base_score,
        "value",
        identity.leaf_name(),
        Some(content),
        request,
    )
}

pub(super) fn exact_definition_chunk_bonus(request: &CodeRetrievalRequest, content: &str) -> f64 {
    if request.code_query_kind != CodeQueryKind::Definition {
        return 0.0;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return 0.0;
    };

    if content
        .lines()
        .map(str::trim)
        .any(|line| declaration_line_defines_identity(line, identity.leaf_name()))
    {
        3.0
    } else {
        0.0
    }
}

fn declaration_line_defines_identity(line: &str, leaf_name: &str) -> bool {
    if !line_contains_identifier(line, leaf_name) {
        return false;
    }
    if line.starts_with("typedef ") || line.contains(" typedef ") {
        return true;
    }
    if line
        .strip_prefix("using ")
        .is_some_and(|remainder| line_starts_with_identifier(remainder, leaf_name))
    {
        return true;
    }

    ["struct ", "class ", "enum ", "union "]
        .into_iter()
        .filter_map(|prefix| line.strip_prefix(prefix))
        .any(|remainder| line_starts_with_identifier(remainder, leaf_name))
}

fn line_starts_with_identifier(line: &str, identifier: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(identifier)
        && trimmed
            .get(identifier.len()..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
}

fn line_contains_identifier(line: &str, identifier: &str) -> bool {
    line.match_indices(identifier).any(|(start, _)| {
        let end = start + identifier.len();
        line.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|c| !is_identifier_char(c))
        }) && line
            .get(end..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
    })
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
