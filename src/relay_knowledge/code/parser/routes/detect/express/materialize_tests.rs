use std::collections::BTreeSet;

use super::materialize_express_routes;
use crate::code::parser::routes::detect::express::{
    DYNAMIC_EXPRESS_MOUNT_PREFIX, ExpressRouteInfo, ExpressRouterMount,
};

fn route(receiver_name: &str, local_url: &str) -> ExpressRouteInfo {
    ExpressRouteInfo {
        receiver_name: receiver_name.to_owned(),
        local_url: local_url.to_owned(),
        http_method: "get".to_owned(),
        handler_name: "listUsers".to_owned(),
        line: 7,
    }
}

#[test]
fn materializes_nested_mount_prefixes_and_deduplicates_routes() {
    let mounts = [
        ExpressRouterMount {
            receiver_name: "app".to_owned(),
            router_name: "api".to_owned(),
            local_prefix: "/api".to_owned(),
        },
        ExpressRouterMount {
            receiver_name: "api".to_owned(),
            router_name: "users".to_owned(),
            local_prefix: "/v1".to_owned(),
        },
    ];

    let routes = materialize_express_routes(
        vec![route("users", "/users"), route("users", "/users")],
        &mounts,
        &BTreeSet::from(["app".to_owned()]),
    );

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/v1/users");
}

#[test]
fn drops_routes_with_dynamic_mount_prefixes() {
    let mounts = [ExpressRouterMount {
        receiver_name: "app".to_owned(),
        router_name: "users".to_owned(),
        local_prefix: DYNAMIC_EXPRESS_MOUNT_PREFIX.to_owned(),
    }];

    let routes = materialize_express_routes(
        vec![route("users", "/users")],
        &mounts,
        &BTreeSet::from(["app".to_owned()]),
    );

    assert!(routes.is_empty());
}
