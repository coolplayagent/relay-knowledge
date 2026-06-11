use rusqlite::types::Value;

use crate::domain::{CodeRepositoryStatus, CodeRetrievalRequest};

use super::escape_sql_like;

pub(in crate::storage::sqlite::code::code_query) fn fts_path_and_language_filter_sql(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    let mut clauses = Vec::new();
    push_path_filter_sql(&mut clauses, "path", &status.path_filters);
    push_path_filter_sql(&mut clauses, "path", &request.repository.path_filters);
    push_query_path_substring_filter_sql(&mut clauses, "path", &request.query_path_substrings);
    push_language_filter_sql(
        &mut clauses,
        "language_id",
        "path",
        &status.language_filters,
    );
    push_language_filter_sql(
        &mut clauses,
        "language_id",
        "path",
        &request.repository.language_filters,
    );
    push_language_filter_sql(
        &mut clauses,
        "language_id",
        "path",
        &request.query_language_filters,
    );
    if request.exclude_generated {
        clauses.push(
            "NOT EXISTS (
                SELECT 1
                FROM code_repository_files generated_file
                WHERE generated_file.source_scope = code_repository_search.source_scope
                  AND generated_file.path = code_repository_search.path
                  AND generated_file.is_generated != 0
            )"
            .to_owned(),
        );
    }
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    }
}

pub(in crate::storage::sqlite::code::code_query) fn path_filter_sql_for_column(
    column: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    let mut clauses = Vec::new();
    push_path_filter_sql(&mut clauses, column, &status.path_filters);
    push_path_filter_sql(&mut clauses, column, &request.repository.path_filters);
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    }
}

pub(in crate::storage::sqlite::code::code_query) fn language_filter_sql_for_column(
    column: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    language_filter_sql_for_columns(column, "path", status, request)
}

pub(in crate::storage::sqlite::code::code_query) fn language_filter_sql_for_columns(
    language_column: &str,
    path_column: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> String {
    let mut clauses = Vec::new();
    push_language_filter_sql(
        &mut clauses,
        language_column,
        path_column,
        &status.language_filters,
    );
    push_language_filter_sql(
        &mut clauses,
        language_column,
        path_column,
        &request.repository.language_filters,
    );
    push_language_filter_sql(
        &mut clauses,
        language_column,
        path_column,
        &request.query_language_filters,
    );
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND {}", clauses.join(" AND "))
    }
}

pub(in crate::storage::sqlite::code::code_query) fn kind_filter_sql_for_column(
    column: &str,
    request: &CodeRetrievalRequest,
) -> String {
    if request.query_kind_filters.is_empty() {
        return String::new();
    }
    let clauses = std::iter::repeat_with(|| format!("{column} = ?"))
        .take(request.query_kind_filters.len())
        .collect::<Vec<_>>();
    format!("AND ({})", clauses.join(" OR "))
}

fn push_path_filter_sql(clauses: &mut Vec<String>, column: &str, filters: &[String]) {
    let clauses_for_filters = filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
        .map(|_| format!("({column} = ? OR {column} LIKE ? ESCAPE '\\')"))
        .collect::<Vec<_>>();
    if !clauses_for_filters.is_empty() {
        clauses.push(format!("({})", clauses_for_filters.join(" OR ")));
    }
}

pub(in crate::storage::sqlite::code::code_query) fn push_query_path_substring_filter_sql(
    clauses: &mut Vec<String>,
    column: &str,
    filters: &[String],
) {
    let clauses_for_filters = filters
        .iter()
        .filter(|filter| !filter.is_empty())
        .map(|_| format!("lower({column}) LIKE ? ESCAPE '\\'"))
        .collect::<Vec<_>>();
    if !clauses_for_filters.is_empty() {
        clauses.push(format!("({})", clauses_for_filters.join(" OR ")));
    }
}

fn push_language_filter_sql(
    clauses: &mut Vec<String>,
    language_column: &str,
    path_column: &str,
    filters: &[String],
) {
    let clauses_for_filters = filters
        .iter()
        .map(|filter| {
            if filter == "cpp" {
                format!(
                    "({language_column} = ? OR ({language_column} = 'c' AND lower({path_column}) LIKE '%.h'))"
                )
            } else {
                format!("{language_column} = ?")
            }
        })
        .collect::<Vec<_>>();
    if !clauses_for_filters.is_empty() {
        clauses.push(format!("({})", clauses_for_filters.join(" OR ")));
    }
}

pub(in crate::storage::sqlite::code::code_query) fn push_path_filter_values(
    values: &mut Vec<Value>,
    filters: &[String],
) {
    for filter in filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
    {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(format!("{}/%", escape_sql_like(&filter))));
    }
}

pub(in crate::storage::sqlite::code::code_query) fn push_query_path_substring_filter_values(
    values: &mut Vec<Value>,
    filters: &[String],
) {
    values.extend(
        filters
            .iter()
            .filter(|filter| !filter.is_empty())
            .map(|filter| {
                Value::Text(format!(
                    "%{}%",
                    escape_sql_like(&filter.to_ascii_lowercase())
                ))
            }),
    );
}

pub(in crate::storage::sqlite::code::code_query) fn push_language_filter_values(
    values: &mut Vec<Value>,
    filters: &[String],
) {
    values.extend(filters.iter().cloned().map(Value::Text));
}

pub(in crate::storage::sqlite::code::code_query) fn push_kind_filter_values(
    values: &mut Vec<Value>,
    request: &CodeRetrievalRequest,
) {
    values.extend(request.query_kind_filters.iter().cloned().map(Value::Text));
}

fn normalized_sql_path_filter(filter: &str) -> Option<String> {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }
    (!filter.is_empty() && filter != ".").then(|| filter.to_owned())
}
