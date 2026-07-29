use super::{route_handler_search_terms, route_http_method_search_terms};

#[test]
fn any_route_method_search_terms_include_concrete_verbs() {
    let terms = route_http_method_search_terms("any");

    assert!(terms.contains("any"));
    assert!(terms.contains("get"));
    assert!(terms.contains("post"));
    assert!(terms.contains("options"));
}

#[test]
fn route_handler_search_terms_split_identifier_parts() {
    let terms = route_handler_search_terms("usersController.listActiveUsers");

    assert!(terms.contains("users"));
    assert!(terms.contains("controller"));
    assert!(terms.contains("list"));
    assert!(terms.contains("active"));
}
