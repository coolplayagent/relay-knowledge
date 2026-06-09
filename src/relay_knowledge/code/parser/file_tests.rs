use crate::domain::CodeRepositoryRegistration;

use super::*;
use std::collections::BTreeSet;

#[test]
fn record_routes_links_records_to_handler_symbols() {
    let mut build = route_test_build();
    build.symbols.push(route_symbol("list-users-symbol", 3, 5));

    record_routes(
        &mut build,
        "src/routes.ts",
        "routes-file",
        "typescript",
        "app.get('/users', listUsers);\napp.post('/users', listUsers);\nfunction listUsers() {}\n",
    );

    assert_eq!(build.routes.len(), 2);
    assert!(
        build
            .routes
            .iter()
            .all(|route| route.handler_symbol_snapshot_id.as_deref() == Some("list-users-symbol"))
    );
    let Some(SymbolRole::RouteHandlers { routes }) = &build.symbols[0].symbol_role else {
        panic!("shared handler should preserve every route role");
    };
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/users" && route.http_method == "get")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.url == "/users" && route.http_method == "post")
    );
}

#[test]
fn record_routes_links_handler_symbols_declared_before_route_registration() {
    let mut build = route_test_build();
    build.symbols.push(route_symbol("before-route", 1, 1));

    record_routes(
        &mut build,
        "src/routes.ts",
        "routes-file",
        "typescript",
        "function listUsers() {}\napp.get('/users', listUsers);\n",
    );

    assert_eq!(
        build.routes[0].handler_symbol_snapshot_id.as_deref(),
        Some("before-route")
    );
    assert!(build.symbols[0].symbol_role.is_some());
}

#[test]
fn record_routes_leaves_anonymous_callbacks_unresolved() {
    let mut build = route_test_build();
    let mut symbol = route_symbol("anonymous-symbol", 1, 1);
    symbol.name = "anonymous".to_owned();
    build.symbols.push(symbol);

    record_routes(
        &mut build,
        "src/routes.ts",
        "routes-file",
        "typescript",
        "app.get('/health', (req, res) => res.end());\n",
    );

    assert!(build.routes[0].handler_symbol_snapshot_id.is_none());
    assert!(build.symbols[0].symbol_role.is_none());
}

#[test]
fn record_routes_links_member_expression_handlers_by_leaf_name() {
    let mut build = route_test_build();
    let mut symbol = route_symbol("bare-list-symbol", 3, 3);
    symbol.name = "list".to_owned();
    symbol.qualified_name = "list".to_owned();
    symbol.signature = "function list()".to_owned();
    build.symbols.push(symbol);

    record_routes(
        &mut build,
        "src/routes.ts",
        "routes-file",
        "typescript",
        "router.get('/users', usersController.list);\nfunction list() {}\n",
    );

    assert_eq!(build.routes[0].handler_name, "usersController.list");
    assert_eq!(
        build.routes[0].handler_symbol_snapshot_id.as_deref(),
        Some("bare-list-symbol")
    );
    assert!(build.symbols[0].symbol_role.is_some());
}

#[test]
fn record_routes_keeps_same_line_route_chain_ids_distinct() {
    let mut build = route_test_build();

    record_routes(
        &mut build,
        "src/routes.ts",
        "routes-file",
        "typescript",
        "router.route('/health').get(requireAuth).get(health);\n",
    );

    assert_eq!(build.routes.len(), 2);
    assert!(build.routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "requireAuth"
    }));
    assert!(build.routes.iter().any(|route| {
        route.url == "/health" && route.http_method == "get" && route.handler_name == "health"
    }));
    let route_ids = build
        .routes
        .iter()
        .map(|route| route.route_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(route_ids.len(), build.routes.len());
}

fn route_test_build() -> SnapshotBuild {
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    )
}

fn route_symbol(
    symbol_snapshot_id: &str,
    line_start: u32,
    line_end: u32,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: "repo://repo/src::routes.ts::listUsers".to_owned(),
        file_id: "routes-file".to_owned(),
        path: "src/routes.ts".to_owned(),
        language_id: "typescript".to_owned(),
        name: "listUsers".to_owned(),
        qualified_name: "listUsers".to_owned(),
        kind: "function".to_owned(),
        signature: "function listUsers()".to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        line_range: RepositoryCodeRange {
            start: line_start,
            end: line_end,
        },
        symbol_role: None,
    }
}
