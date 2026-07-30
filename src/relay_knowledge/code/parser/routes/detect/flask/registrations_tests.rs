use std::collections::BTreeMap;

use super::{
    apply_flask_methods_decorator, bind_pending_routes_to_python_function, parse_flask_decorator,
    parse_python_add_url_rule,
};
use crate::code::parser::routes::detect::ANONYMOUS_ROUTE_HANDLER_NAME;

#[test]
fn parses_route_decorators_and_shorthand_methods() {
    let routers = BTreeMap::new();
    let route = parse_flask_decorator(r#"@app.route("/items", methods=["POST"])"#, &routers)
        .expect("Flask route");
    let shorthand =
        parse_flask_decorator(r#"@app.patch("/items/{id}")"#, &routers).expect("shorthand route");

    assert_eq!(route.local_url, "/items");
    assert_eq!(route.methods, vec!["post".to_owned()]);
    assert_eq!(shorthand.methods, vec!["patch".to_owned()]);
}

#[test]
fn methods_decorators_override_the_pending_route() {
    let mut routes = vec![
        parse_flask_decorator(r#"@app.route("/items")"#, &BTreeMap::new()).expect("pending route"),
    ];

    assert!(apply_flask_methods_decorator(
        r#"@app.methods(["PUT", "DELETE"])"#,
        &mut routes,
    ));
    assert_eq!(
        routes[0].methods,
        vec!["put".to_owned(), "delete".to_owned()]
    );
}

#[test]
fn binds_stacked_routes_to_async_python_functions() {
    let mut routes = vec![
        parse_flask_decorator(r#"@app.get("/items")"#, &BTreeMap::new()).expect("first route"),
        parse_flask_decorator(r#"@app.post("/items")"#, &BTreeMap::new()).expect("second route"),
    ];

    let bindings = bind_pending_routes_to_python_function("async def items():", &mut routes, 14)
        .expect("function binding");

    assert!(routes.is_empty());
    assert_eq!(bindings.len(), 2);
    assert!(
        bindings
            .iter()
            .all(|binding| binding.handler_name == "items")
    );
    assert!(bindings.iter().all(|binding| binding.line == 14));
}

#[test]
fn parses_add_url_rule_keyword_and_positional_handlers() {
    let keyword = parse_python_add_url_rule(
        r#"app.add_url_rule("/items", view_func=views.list_items, methods=["GET", "POST"])"#,
        &BTreeMap::new(),
        8,
    )
    .expect("keyword handler");
    let positional = parse_python_add_url_rule(
        r#"app.add_url_rule("/health", "health", handlers.health)"#,
        &BTreeMap::new(),
        20,
    )
    .expect("positional handler");

    assert_eq!(keyword.len(), 2);
    assert!(
        keyword
            .iter()
            .all(|binding| binding.handler_name == "list_items")
    );
    assert_eq!(keyword[0].line, 9);
    assert_eq!(positional[0].handler_name, "health");
    assert_eq!(positional[0].http_method, "get");
}

#[test]
fn assigns_anonymous_handler_when_add_url_rule_has_no_named_target() {
    let bindings = parse_python_add_url_rule(r#"app.add_url_rule("/health")"#, &BTreeMap::new(), 0)
        .expect("anonymous rule");

    assert_eq!(bindings[0].handler_name, ANONYMOUS_ROUTE_HANDLER_NAME);
}

#[test]
fn rejects_non_route_decorators_and_non_function_bindings() {
    assert!(parse_flask_decorator("@cache.memoize(60)", &BTreeMap::new()).is_none());
    assert!(parse_flask_decorator("@app.route", &BTreeMap::new()).is_none());
    let mut routes = vec![
        parse_flask_decorator(r#"@app.get("/items")"#, &BTreeMap::new()).expect("pending route"),
    ];

    assert!(
        bind_pending_routes_to_python_function("items = service.list()", &mut routes, 1).is_none()
    );
    assert_eq!(routes.len(), 1);
}
