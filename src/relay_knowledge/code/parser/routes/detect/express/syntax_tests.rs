use std::collections::BTreeSet;

use super::{
    express_http_method, express_receiver_name, express_route_urls, express_router_name_is_router,
    javascript_call_end, javascript_top_level_arguments, merge_url_parts,
};

#[test]
fn route_urls_keep_static_paths_and_drop_dynamic_templates() {
    assert_eq!(
        express_route_urls("['/users', `/items`, `${prefix}/dynamic`], handler)"),
        vec!["/users".to_owned(), "/items".to_owned()]
    );
    assert!(express_route_urls("dynamicPath, handler)").is_empty());
}

#[test]
fn call_scanning_respects_nested_arguments_and_quotes() {
    let arguments = "'/users', middleware({ value: ')' }), [first, second]), trailing";

    let call_end = javascript_call_end(arguments).expect("call should close");

    assert_eq!(
        &arguments[..call_end],
        "'/users', middleware({ value: ')' }), [first, second])"
    );
    assert_eq!(
        javascript_top_level_arguments(arguments),
        vec!["'/users'", "middleware({ value: ')' })", "[first, second]"]
    );
}

#[test]
fn receiver_and_method_normalization_follow_express_contract() {
    let routers = BTreeSet::from(["users$".to_owned()]);

    assert_eq!(
        express_receiver_name("namespace.users$"),
        Some("users$".to_owned())
    );
    assert!(express_router_name_is_router("users$", &routers));
    assert!(express_router_name_is_router("APP", &BTreeSet::new()));
    assert_eq!(express_http_method("ALL"), Some("any".to_owned()));
    assert_eq!(express_http_method("connect"), None);
}

#[test]
fn url_joining_normalizes_boundary_slashes() {
    assert_eq!(merge_url_parts("/api/", "/users"), "/api/users");
    assert_eq!(merge_url_parts("", "health"), "/health");
    assert_eq!(merge_url_parts("/api", ""), "/api");
}
