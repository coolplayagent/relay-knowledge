//! Direct contracts for symbol role serialization and search fields.

use crate::domain::{RouteHandlerRole, SymbolRole};

use super::symbol_role_search_fields;

#[test]
fn symbol_role_search_fields_include_every_route_handler_binding() {
    let role = Some(SymbolRole::RouteHandlers {
        routes: vec![
            RouteHandlerRole {
                url: "/items".to_owned(),
                http_method: "get".to_owned(),
            },
            RouteHandlerRole {
                url: "/items".to_owned(),
                http_method: "post".to_owned(),
            },
        ],
    });

    let (kind, urls, methods) = symbol_role_search_fields(&role);

    assert_eq!(kind, "route_handler");
    assert!(urls.contains("/items"));
    assert!(methods.contains("get"));
    assert!(methods.contains("post"));
}
