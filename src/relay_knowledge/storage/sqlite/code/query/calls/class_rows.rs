//! Class call selection uses existing name indexes and a shared SQLite work budget.

use rusqlite::{Connection, ErrorCode, params_from_iter, types::Value};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::{
    super::{prepare_code_search_statement, relevance::*, required_scope, rows::CallRow},
    class_members,
    identity_query::call_identity_candidate_limit,
    row_store::{call_rows_sql, row_to_call},
};
use crate::{
    domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalRequest},
    storage::StorageError,
};

const MAX_WORK_CALLBACKS: usize = 4096;

struct WorkBudget<'a>(&'a Connection);

impl Drop for WorkBudget<'_> {
    fn drop(&mut self) {
        self.0.progress_handler(0, None::<fn() -> bool>);
    }
}

pub(super) fn search(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Option<Vec<CallRow>>, StorageError> {
    if request.query.split_whitespace().count() != 1 {
        return Ok(None);
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return Ok(None);
    };
    let counter = Arc::new(AtomicUsize::new(0));
    connection.progress_handler(
        1000,
        Some(move || counter.fetch_add(1, Ordering::Relaxed) >= MAX_WORK_CALLBACKS),
    );
    let _budget = WorkBudget(connection);
    let result = class_members::resolve(connection, required_scope(status)?, &identity).and_then(
        |members| {
            members
                .map(|members| select_rows(connection, status, request, members))
                .transpose()
        },
    );
    match result {
        Err(StorageError::Sqlite(rusqlite::Error::SqliteFailure(error, _)))
            if error.code == ErrorCode::OperationInterrupted =>
        {
            Err(class_members::capacity("SQLite execution budget exhausted"))
        }
        result => result,
    }
}

fn select_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    members: Vec<class_members::ClassMember>,
) -> Result<Vec<CallRow>, StorageError> {
    if members.is_empty() {
        return Ok(Vec::new());
    }
    let names = members
        .iter()
        .map(|member| member.name.as_str())
        .collect::<BTreeSet<_>>();
    let (name_column, snapshot_column, result_identity) = match request.code_query_kind {
        CodeQueryKind::Callers => (
            "c.callee_name",
            "c.callee_symbol_snapshot_id",
            "caller.canonical_symbol_id",
        ),
        _ => (
            "c.caller_name",
            "c.caller_symbol_snapshot_id",
            "COALESCE(callee.canonical_symbol_id, c.target_hint, c.callee_name)",
        ),
    };
    let path_filter = path_filter_sql_for_column("c.path", status, request);
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let mut filters = Vec::new();
    push_query_path_substring_filter_sql(&mut filters, "c.path", &request.query_path_substrings);
    push_query_path_substring_filter_sql(
        &mut filters,
        result_identity,
        &request.query_name_substrings,
    );
    let inline_filter = if filters.is_empty() {
        String::new()
    } else {
        format!("AND {}", filters.join(" AND "))
    };
    let generated_filter = if request.exclude_generated {
        "AND f.is_generated = 0"
    } else {
        ""
    };
    let sql = call_rows_sql(&format!(
        "AND {name_column} IN ({}) AND {snapshot_column} IN ({})
         {path_filter} {language_filter} {inline_filter} {generated_filter}",
        vec!["?"; names.len()].join(","),
        vec!["?"; members.len()].join(","),
    ));
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(names.into_iter().map(|name| Value::Text(name.to_owned())));
    values.extend(
        members
            .into_iter()
            .map(|member| Value::Text(member.snapshot)),
    );
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    push_query_path_substring_filter_values(&mut values, &request.query_path_substrings);
    push_query_path_substring_filter_values(&mut values, &request.query_name_substrings);
    values.push(Value::Integer(call_identity_candidate_limit(request) as i64));
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    statement
        .query_map(params_from_iter(values), row_to_call)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "class_rows_tests.rs"]
mod tests;
