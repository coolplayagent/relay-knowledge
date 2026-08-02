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
