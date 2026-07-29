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
mod tests {
    use rusqlite::{Connection, types::Value};

    use crate::domain::{
        CodeRepositorySelector, CodeRouteRecord, CodebaseViewKind, CodebaseViewRequest,
        FreshnessPolicy, RepositoryCodeRange,
    };

    use super::{append_call_focus_filters, call_focus_paths};

    #[test]
    fn process_flow_focus_includes_resolved_handler_paths() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE code_repository_symbols (
                    source_scope TEXT NOT NULL,
                    symbol_snapshot_id TEXT NOT NULL,
                    path TEXT NOT NULL
                );
                INSERT INTO code_repository_symbols VALUES
                    ('scope', 'symbol:handler', 'src/controllers/users.ts');
                ",
            )
            .unwrap();
        let request = request(CodebaseViewKind::ProcessFlow);
        let route = route("src/routes.ts", Some("symbol:handler"));

        let focus = call_focus_paths(&connection, "scope", &request, &[route], 20).unwrap();
        let mut sql = String::new();
        let mut values = Vec::new();
        append_call_focus_filters(&mut sql, &mut values, &focus);

        assert!(sql.contains("call.path = ?"));
        assert!(values.contains(&Value::Text("src/routes.ts".to_owned())));
        assert!(values.contains(&Value::Text("src/controllers/users.ts".to_owned())));
    }

    fn request(view_kind: CodebaseViewKind) -> CodebaseViewRequest {
        CodebaseViewRequest::new(
            CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
            view_kind,
            FreshnessPolicy::AllowStale,
            20,
            Vec::new(),
        )
        .unwrap()
    }

    fn route(path: &str, handler_symbol_snapshot_id: Option<&str>) -> CodeRouteRecord {
        CodeRouteRecord {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            route_id: format!("route:{path}"),
            file_id: format!("file:{path}"),
            path: path.to_owned(),
            language_id: "typescript".to_owned(),
            url: "/users".to_owned(),
            http_method: "GET".to_owned(),
            handler_name: "listUsers".to_owned(),
            handler_symbol_snapshot_id: handler_symbol_snapshot_id.map(str::to_owned),
            framework: "fixture".to_owned(),
            line_range: RepositoryCodeRange { start: 1, end: 1 },
        }
    }
}
