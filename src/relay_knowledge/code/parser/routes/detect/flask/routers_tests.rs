use std::collections::BTreeMap;

use super::{
    apply_python_include_router_prefix, apply_python_register_blueprint_prefix,
    merge_python_router_declaration, parse_python_router_prefix, route_framework,
};
use crate::code::parser::routes::detect::flask::arguments::DYNAMIC_PYTHON_MOUNT_PREFIX;

#[test]
fn parses_typed_fastapi_router_declarations() {
    let (name, router) =
        parse_python_router_prefix(r#"items_router: APIRouter = APIRouter(prefix="/items")"#)
            .expect("typed router declaration");

    assert_eq!(name, "items_router");
    assert_eq!(router.local_prefix, "/items");
    assert_eq!(router.framework, "fastapi");
    assert!(router.mount_required);
    assert!(router.cross_file_mount_candidate);
}

#[test]
fn classifies_dynamic_router_prefixes_without_guessing_values() {
    let (_, router) = parse_python_router_prefix("router = APIRouter(prefix=SETTINGS.api_prefix)")
        .expect("dynamic router declaration");

    assert_eq!(router.local_prefix, DYNAMIC_PYTHON_MOUNT_PREFIX);
}

#[test]
fn late_declarations_preserve_observed_include_mounts() {
    let mut routers = BTreeMap::new();
    assert!(apply_python_include_router_prefix(
        "app.include_router(items_router, prefix='/api')",
        &mut routers,
    ));
    let (name, router) = parse_python_router_prefix("items_router = APIRouter(prefix='/v1')")
        .expect("router declaration");

    merge_python_router_declaration(&mut routers, name, router);

    let router = routers.get("items_router").expect("merged router");
    assert_eq!(router.local_prefix, "/v1");
    assert_eq!(
        router.mount_prefixes.iter().cloned().collect::<Vec<_>>(),
        vec!["/api".to_owned()]
    );
}

#[test]
fn records_blueprint_mounts_and_resolves_frameworks() {
    let mut routers = BTreeMap::new();
    assert!(apply_python_register_blueprint_prefix(
        "app.register_blueprint(admin, url_prefix='/admin')",
        &mut routers,
    ));

    assert_eq!(
        route_framework("admin.route", Some("admin"), &routers),
        "flask"
    );
    assert_eq!(
        routers
            .get("admin")
            .expect("registered blueprint")
            .mount_prefixes
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["/admin".to_owned()]
    );
}

#[test]
fn accepts_blueprints_and_rejects_unknown_factories_or_invalid_names() {
    assert!(
        parse_python_router_prefix("api = Blueprint('api', __name__, url_prefix='/v1')").is_some()
    );
    assert!(parse_python_router_prefix("router = CustomRouter()").is_none());
    assert!(parse_python_router_prefix("items-router = APIRouter()").is_none());
}

#[test]
fn resolves_unbound_api_route_as_fastapi() {
    assert_eq!(
        route_framework("app.api_route", Some("app"), &BTreeMap::new()),
        "fastapi"
    );
    assert_eq!(
        route_framework("app.route", Some("app"), &BTreeMap::new()),
        "flask"
    );
}
