use super::{
    DYNAMIC_PYTHON_MOUNT_PREFIX, extract_methods_from_flask_args,
    extract_python_add_url_rule_positional_handler, extract_python_keyword_value,
    extract_python_route_path, extract_python_router_argument, parse_flask_methods_decorator,
    python_handler_name_from_value, python_prefix_argument, trim_one_trailing_paren,
};

#[test]
fn splits_nested_keyword_values_without_losing_following_arguments() {
    let args = r#"path="/items", defaults={"filter": (1, 2)}, methods=["GET", "POST"]"#;

    assert_eq!(
        extract_python_keyword_value(args, "defaults"),
        Some(r#"{"filter": (1, 2)}"#)
    );
    assert_eq!(
        extract_methods_from_flask_args(args),
        vec!["get".to_owned(), "post".to_owned()]
    );
}

#[test]
fn extracts_static_and_dynamic_prefix_arguments() {
    assert_eq!(python_prefix_argument(r#"prefix="/api""#, "prefix"), "/api");
    assert_eq!(
        python_prefix_argument("prefix=SETTINGS.api_prefix", "prefix"),
        DYNAMIC_PYTHON_MOUNT_PREFIX
    );
    assert_eq!(python_prefix_argument("tags=['items']", "prefix"), "");
}

#[test]
fn accepts_keyword_or_positional_router_identifiers() {
    assert_eq!(
        extract_python_router_argument("router=items_router, prefix='/api'", "router"),
        Some("items_router".to_owned())
    );
    assert_eq!(
        extract_python_router_argument("items_router, prefix='/api'", "router"),
        Some("items_router".to_owned())
    );
    assert_eq!(
        extract_python_router_argument("lambda: router", "router"),
        None
    );
}

#[test]
fn extracts_route_path_with_keyword_precedence() {
    assert_eq!(
        extract_python_route_path(r#""/fallback", path="/preferred""#),
        Some("/preferred".to_owned())
    );
    assert_eq!(
        extract_python_route_path(r#"rule="/rule""#),
        Some("/rule".to_owned())
    );
}

#[test]
fn extracts_named_handlers_and_rejects_inline_callbacks() {
    assert_eq!(
        python_handler_name_from_value("views.ItemView.as_view('items')"),
        Some("ItemView".to_owned())
    );
    assert_eq!(python_handler_name_from_value("lambda: response"), None);
    assert_eq!(
        extract_python_add_url_rule_positional_handler(
            r#""/items", "items", handlers.list_items, methods=["GET"]"#
        ),
        Some("list_items".to_owned())
    );
}

#[test]
fn parses_methods_decorators_and_default_route_methods() {
    assert_eq!(
        parse_flask_methods_decorator(r#"@app.methods(["POST", "PATCH"])"#),
        Some(vec!["post".to_owned(), "patch".to_owned()])
    );
    assert_eq!(
        extract_methods_from_flask_args(r#""/items""#),
        vec!["get".to_owned()]
    );
    assert_eq!(
        extract_methods_from_flask_args(r#""/items", methods=METHODS"#),
        vec!["any".to_owned()]
    );
    assert!(parse_flask_methods_decorator("@app.methods").is_none());
}

#[test]
fn removes_at_most_one_trailing_call_parenthesis() {
    assert_eq!(trim_one_trailing_paren("call())  "), "call()");
    assert_eq!(trim_one_trailing_paren("value"), "value");
}
