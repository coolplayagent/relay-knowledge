use rusqlite::types::Value;

use super::code_query_support::{
    candidate_patterns, push_language_filter_values, push_path_filter_values,
    push_query_path_substring_filter_values,
};
use crate::domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalRequest};

pub(super) fn fts_values_for_limited_with_language_and_call_direction(
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
    push_call_direction_filter_values(&mut values, request);
    values.push(Value::Integer(fts_limit as i64));
    values.push(Value::Integer(limit as i64));

    values
}

pub(super) fn call_direction_fts_filter_sql(request: &CodeRetrievalRequest) -> String {
    let Some(surface) = call_direction_filter_surface(request.code_query_kind) else {
        return String::new();
    };
    let patterns = candidate_patterns(&request.query, 8);
    if patterns.is_empty() {
        return String::new();
    }
    let clauses = patterns
        .iter()
        .map(|_| format!("{surface} LIKE ? ESCAPE '\\'"))
        .collect::<Vec<_>>()
        .join(" OR ");

    format!(
        "AND EXISTS (
            SELECT 1
            FROM code_repository_calls call_filter
            LEFT JOIN code_repository_symbols caller_filter
              ON caller_filter.source_scope = call_filter.source_scope
             AND caller_filter.symbol_snapshot_id = call_filter.caller_symbol_snapshot_id
            LEFT JOIN code_repository_symbols callee_filter
              ON callee_filter.source_scope = call_filter.source_scope
             AND callee_filter.symbol_snapshot_id = call_filter.callee_symbol_snapshot_id
            WHERE call_filter.source_scope = code_repository_search.source_scope
              AND call_filter.call_id = code_repository_search.record_id
              AND ({clauses})
        )"
    )
}

fn push_call_direction_filter_values(values: &mut Vec<Value>, request: &CodeRetrievalRequest) {
    if call_direction_filter_surface(request.code_query_kind).is_none() {
        return;
    }
    values.extend(
        candidate_patterns(&request.query, 8)
            .into_iter()
            .map(Value::Text),
    );
}

fn call_direction_filter_surface(kind: CodeQueryKind) -> Option<&'static str> {
    match kind {
        CodeQueryKind::Callers => {
            Some("lower(call_filter.callee_name || ' ' || coalesce(callee_filter.signature, ''))")
        }
        CodeQueryKind::Callees => Some(
            "lower(coalesce(call_filter.caller_name, '') || ' ' || coalesce(caller_filter.signature, ''))",
        ),
        _ => None,
    }
}
