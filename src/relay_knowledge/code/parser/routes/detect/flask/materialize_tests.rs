use std::collections::{BTreeMap, BTreeSet};

use super::{PythonRouteBinding, materialize_python_routes};
use crate::code::parser::routes::detect::flask::arguments::DYNAMIC_PYTHON_MOUNT_PREFIX;
use crate::code::parser::routes::detect::flask::routers::PythonRouterInfo;

fn route_binding(receiver_name: Option<&str>) -> PythonRouteBinding {
    PythonRouteBinding {
        receiver_name: receiver_name.map(str::to_owned),
        local_url: "/items".to_owned(),
        http_method: "get".to_owned(),
        handler_name: "list_items".to_owned(),
        framework: "flask".to_owned(),
        line: 7,
    }
}

#[test]
fn materializes_root_routes_and_deduplicates_bindings() {
    let routes = materialize_python_routes(
        vec![route_binding(None), route_binding(None)],
        &BTreeMap::new(),
    );

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/items");
    assert_eq!(routes[0].framework, "flask");
}

#[test]
fn joins_mount_and_local_router_prefixes() {
    let routers = BTreeMap::from([(
        "items".to_owned(),
        PythonRouterInfo {
            local_prefix: "/v1".to_owned(),
            mount_prefixes: BTreeSet::from(["/api".to_owned()]),
            framework: "fastapi".to_owned(),
            mount_required: true,
            cross_file_mount_candidate: false,
        },
    )]);

    let routes = materialize_python_routes(vec![route_binding(Some("items"))], &routers);

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/api/v1/items");
    assert_eq!(routes[0].framework, "fastapi");
}

#[test]
fn preserves_cross_file_mount_candidates_with_placeholder_prefixes() {
    let routers = BTreeMap::from([(
        "items_router".to_owned(),
        PythonRouterInfo {
            local_prefix: "/v1".to_owned(),
            mount_prefixes: BTreeSet::new(),
            framework: "fastapi".to_owned(),
            mount_required: true,
            cross_file_mount_candidate: true,
        },
    )]);

    let routes = materialize_python_routes(vec![route_binding(Some("items_router"))], &routers);

    assert_eq!(routes[0].url, "/:mount/v1/items");
}

#[test]
fn drops_dynamic_router_and_mount_prefixes() {
    let routers = BTreeMap::from([
        (
            "dynamic_router".to_owned(),
            PythonRouterInfo {
                local_prefix: DYNAMIC_PYTHON_MOUNT_PREFIX.to_owned(),
                mount_prefixes: BTreeSet::new(),
                framework: "fastapi".to_owned(),
                mount_required: true,
                cross_file_mount_candidate: false,
            },
        ),
        (
            "dynamic_mount".to_owned(),
            PythonRouterInfo {
                local_prefix: "/v1".to_owned(),
                mount_prefixes: BTreeSet::from([DYNAMIC_PYTHON_MOUNT_PREFIX.to_owned()]),
                framework: "fastapi".to_owned(),
                mount_required: true,
                cross_file_mount_candidate: false,
            },
        ),
    ]);

    let routes = materialize_python_routes(
        vec![
            route_binding(Some("dynamic_router")),
            route_binding(Some("dynamic_mount")),
        ],
        &routers,
    );

    assert!(routes.is_empty());
}
