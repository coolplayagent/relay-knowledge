use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{CodeRouteRecord, CodebaseViewKind, CodebaseViewRequest},
    storage::StorageError,
};

use super::append_path_filter_set;

pub(super) struct CallFocusPaths {
    caller_paths: Vec<String>,
    callee_paths: Vec<String>,
}

impl CallFocusPaths {
    fn empty() -> Self {
        Self {
            caller_paths: Vec::new(),
            callee_paths: Vec::new(),
        }
    }
}

pub(super) fn call_focus_paths(
    connection: &Connection,
    source_scope: &str,
    request: &CodebaseViewRequest,
    routes: &[CodeRouteRecord],
    limit: usize,
) -> Result<CallFocusPaths, StorageError> {
    match request.view_kind {
        CodebaseViewKind::ProcessFlow => {
            let mut caller_paths = dedupe_paths(routes.iter().map(|route| route.path.as_str()));
            for path in resolved_handler_paths(connection, source_scope, routes, limit)? {
                push_unique_path(&mut caller_paths, path);
            }
            Ok(CallFocusPaths {
                caller_paths,
                callee_paths: Vec::new(),
            })
        }
        CodebaseViewKind::AffectedScope => {
            let paths = dedupe_paths(request.changed_paths.iter().map(String::as_str));
            Ok(CallFocusPaths {
                caller_paths: paths.clone(),
                callee_paths: paths,
            })
        }
        CodebaseViewKind::ArchitectureLayers
        | CodebaseViewKind::BusinessDomains
        | CodebaseViewKind::DependencyTour => Ok(CallFocusPaths::empty()),
    }
}

pub(super) fn append_call_focus_filters(
    sql: &mut String,
    values: &mut Vec<Value>,
    focus: &CallFocusPaths,
) {
    if focus.caller_paths.is_empty() && focus.callee_paths.is_empty() {
        return;
    }
    sql.push_str(" AND (");
    let has_clause = append_path_filter_set(sql, values, "call.path", &focus.caller_paths, false);
    append_path_filter_set(sql, values, "callee.path", &focus.callee_paths, has_clause);
    sql.push(')');
}

fn resolved_handler_paths(
    connection: &Connection,
    source_scope: &str,
    routes: &[CodeRouteRecord],
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    let symbol_ids = routes
        .iter()
        .filter_map(|route| route.handler_symbol_snapshot_id.as_deref())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    if symbol_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut sql = "
        SELECT DISTINCT path
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND symbol_snapshot_id IN (
        "
    .to_owned();
    let mut values = vec![Value::Text(source_scope.to_owned())];
    for (index, symbol_id) in symbol_ids.iter().enumerate() {
        if index > 0 {
            sql.push_str(", ");
        }
        sql.push('?');
        values.push(Value::Text((*symbol_id).to_owned()));
    }
    sql.push_str(") ORDER BY path ASC LIMIT ?");
    values.push(Value::Integer(limit as i64));

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| row.get(0))?;
    rows.map(|row| row.map_err(StorageError::from)).collect()
}

fn dedupe_paths<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut deduped = Vec::new();
    for path in paths.filter_map(normalized_path_filter) {
        push_unique_path(&mut deduped, path);
    }
    deduped
}

fn push_unique_path(paths: &mut Vec<String>, path: String) {
    if path != "." && !path.is_empty() && !paths.contains(&path) {
        paths.push(path);
    }
}

fn normalized_path_filter(filter: &str) -> Option<String> {
    let mut filter = filter.replace('\\', "/");
    while filter.ends_with('/') {
        filter.pop();
    }
    while filter.starts_with("./") {
        filter.drain(..2);
    }
    (!filter.is_empty()).then_some(filter)
}

#[cfg(test)]
#[path = "call_focus_tests.rs"]
mod tests;
