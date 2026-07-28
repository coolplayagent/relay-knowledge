//! Plans bounded SQL and FTS candidate layers for code retrieval.

use rusqlite::types::Value;

use crate::domain::{
    CodeQueryKind, CodeRepositoryStatus, CodeRetrievalLayer, CodeRetrievalRequest,
};

use super::super::super::code_query_hits::chunk_layers;
use super::{
    filters::{
        push_language_filter_values, push_path_filter_values,
        push_query_path_substring_filter_values,
    },
    symbol_identity::SymbolIdentityQuery,
    tokens::escape_sql_like,
};

#[cfg(test)]
use super::super::MAX_CANDIDATE_BIND_VALUES;

#[cfg(test)]
pub(in crate::storage::sqlite::code::code_query) fn candidate_condition(
    fields: &[&str],
    query: &str,
) -> (String, Vec<Value>) {
    let max_patterns = (MAX_CANDIDATE_BIND_VALUES / fields.len().max(1)).max(1);
    let patterns = candidate_patterns(query, max_patterns);
    if patterns.is_empty() {
        return ("1 = 1".to_owned(), Vec::new());
    }

    let mut values = Vec::new();
    let groups = patterns
        .into_iter()
        .map(|pattern| {
            let clauses = fields
                .iter()
                .map(|field| {
                    values.push(Value::Text(pattern.clone()));
                    format!("{field} LIKE ?")
                })
                .collect::<Vec<_>>();
            format!("({})", clauses.join(" OR "))
        })
        .collect::<Vec<_>>();

    (groups.join(" OR "), values)
}

pub(in crate::storage::sqlite::code::code_query) fn candidate_patterns(
    query: &str,
    max_patterns: usize,
) -> Vec<String> {
    let mut patterns = Vec::new();
    for token in fts_query_terms(query) {
        let token = escape_sql_like(&token.to_lowercase());
        if token.is_empty() {
            continue;
        }
        let pattern = format!("%{token}%");
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
        if patterns.len() >= max_patterns {
            break;
        }
    }

    patterns
}

pub(in crate::storage::sqlite::code::code_query) fn fts_query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Copy)]
pub(in crate::storage::sqlite::code::code_query) enum CandidateLayer {
    Symbol,
    Reference,
    Call,
    Import,
    Sbom,
    Chunk,
}

pub(in crate::storage::sqlite::code::code_query) fn candidate_limit(
    request: &CodeRetrievalRequest,
    layer: CandidateLayer,
) -> usize {
    let requested = request.limit.max(1);
    let (multiplier, minimum, maximum) = match layer {
        CandidateLayer::Symbol => (40usize, 200usize, 800usize),
        CandidateLayer::Reference => (35, 200, 700),
        CandidateLayer::Call
            if matches!(
                request.code_query_kind,
                CodeQueryKind::Callers | CodeQueryKind::Callees
            ) =>
        {
            (100, 500, 1000)
        }
        CandidateLayer::Call => (40, 250, 800),
        CandidateLayer::Import => (35, 200, 700),
        CandidateLayer::Sbom => (35, 200, 700),
        CandidateLayer::Chunk => (45, 300, 900),
    };

    requested.saturating_mul(multiplier).clamp(minimum, maximum)
}

pub(in crate::storage::sqlite::code::code_query) fn chunk_layers_for_request(
    request: &CodeRetrievalRequest,
    parse_status: &str,
) -> Vec<CodeRetrievalLayer> {
    let mut layers = chunk_layers(parse_status);
    if request.code_query_kind == CodeQueryKind::References
        && SymbolIdentityQuery::from_query(&request.query).is_some()
        && !layers.contains(&CodeRetrievalLayer::TextFallback)
    {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}

pub(in crate::storage::sqlite::code::code_query) fn fts_values_for_limited_with_language(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    fts_query: &str,
    fts_limit: usize,
    limit: usize,
) -> Vec<Value> {
    let mut values = vec![
        Value::Text(source_scope.to_owned()),
        Value::Text(fts_query.to_owned()),
        Value::Text(source_scope.to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_query_path_substring_filter_values(&mut values, &request.query_path_substrings);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    values.push(Value::Integer(fts_limit as i64));
    values.push(Value::Integer(limit as i64));

    values
}
