use std::collections::BTreeSet;

use super::{
    SpringClassPrefix, pending_request_mapping_can_be_prefix, pending_request_mapping_prefixes,
    record_spring_pending_routes,
};
use crate::code::parser::routes::detect::spring::annotations::{
    SpringAnnotationKind, SpringPendingAnnotation,
};

#[test]
fn request_mapping_prefixes_require_only_request_mapping_annotations() {
    let request_mappings = vec![
        annotation("any", "/api", SpringAnnotationKind::RequestMapping),
        annotation("any", "/api", SpringAnnotationKind::RequestMapping),
    ];

    assert!(pending_request_mapping_can_be_prefix(&request_mappings));
    let prefixes = pending_request_mapping_prefixes(&request_mappings);
    assert_eq!(prefixes.len(), 1);
    assert_eq!(prefixes[0].url, "/api");

    let method_mapping = vec![annotation(
        "get",
        "/users",
        SpringAnnotationKind::MethodMapping,
    )];
    assert!(!pending_request_mapping_can_be_prefix(&method_mapping));
}

#[test]
fn materializes_prefixes_methods_and_deduplicates_route_facts() {
    let prefixes = vec![SpringClassPrefix {
        url: "/api".to_owned(),
        http_method: "post".to_owned(),
    }];
    let mut pending = vec![
        annotation("get", "/users", SpringAnnotationKind::MethodMapping),
        annotation("get", "/users", SpringAnnotationKind::MethodMapping),
    ];
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();

    record_spring_pending_routes(
        &mut routes,
        &mut seen,
        &prefixes,
        &mut pending,
        "listUsers".to_owned(),
        12,
    );

    assert!(pending.is_empty());
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].url, "/api/users");
    assert_eq!(routes[0].http_method, "post");
    assert_eq!(routes[1].http_method, "get");
    assert!(
        routes
            .iter()
            .all(|route| route.handler_name == "listUsers" && route.line == 12)
    );
}

#[test]
fn materializes_root_routes_without_a_class_prefix() {
    let mut pending = vec![annotation("get", "", SpringAnnotationKind::MethodMapping)];
    let mut routes = Vec::new();
    let mut seen = BTreeSet::new();

    record_spring_pending_routes(
        &mut routes,
        &mut seen,
        &[],
        &mut pending,
        "root".to_owned(),
        4,
    );

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].url, "/");
    assert_eq!(routes[0].http_method, "get");
}

fn annotation(http_method: &str, url: &str, kind: SpringAnnotationKind) -> SpringPendingAnnotation {
    SpringPendingAnnotation {
        http_method: http_method.to_owned(),
        url: url.to_owned(),
        kind,
    }
}
