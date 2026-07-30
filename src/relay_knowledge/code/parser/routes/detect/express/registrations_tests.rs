use std::collections::BTreeSet;

use super::{ExpressRouteInfo, record_express_method_calls, record_express_route_chain};
use crate::code::parser::routes::detect::ANONYMOUS_ROUTE_HANDLER_NAME;

fn router_names() -> BTreeSet<String> {
    BTreeSet::from(["app".to_owned(), "router".to_owned()])
}

#[test]
fn records_each_method_in_a_route_chain() {
    let mut routes = Vec::new();

    let recorded = record_express_route_chain(
        "router.route('/users').get(listUsers).post(createUser);",
        17,
        &router_names(),
        &mut routes,
    );

    assert!(recorded);
    assert_route(&routes[0], "router", "/users", "get", "listUsers", 17);
    assert_route(&routes[1], "router", "/users", "post", "createUser", 17);
}

#[test]
fn records_direct_methods_and_anonymous_callbacks() {
    let mut routes = Vec::new();

    let recorded = record_express_method_calls(
        "app.get('/ready', middleware, ready); app.post('/users', (req, res) => res.send());",
        8,
        &router_names(),
        &mut routes,
    );

    assert!(recorded);
    assert_route(&routes[0], "app", "/ready", "get", "ready", 8);
    assert_route(
        &routes[1],
        "app",
        "/users",
        "post",
        ANONYMOUS_ROUTE_HANDLER_NAME,
        8,
    );
}

#[test]
fn ignores_methods_on_unregistered_receivers() {
    let mut routes = Vec::new();

    let recorded = record_express_method_calls(
        "service.get('/internal', internalHandler);",
        3,
        &router_names(),
        &mut routes,
    );

    assert!(!recorded);
    assert!(routes.is_empty());
}

fn assert_route(
    route: &ExpressRouteInfo,
    receiver_name: &str,
    local_url: &str,
    http_method: &str,
    handler_name: &str,
    line: usize,
) {
    assert_eq!(route.receiver_name, receiver_name);
    assert_eq!(route.local_url, local_url);
    assert_eq!(route.http_method, http_method);
    assert_eq!(route.handler_name, handler_name);
    assert_eq!(route.line, line);
}
