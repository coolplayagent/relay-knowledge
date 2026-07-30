use super::{
    SpringAnnotationKind, parse_spring_route_annotation, spring_route_annotations_and_tail,
};

#[test]
fn parses_qualified_shorthand_mapping_paths() {
    let annotations = parse_spring_route_annotation(
        r#"@org.springframework.web.bind.annotation.GetMapping({"/users", "/members"})"#,
    )
    .expect("Spring mapping");

    assert_eq!(annotations.len(), 2);
    assert_eq!(annotations[0].http_method, "get");
    assert_eq!(annotations[0].url, "/users");
    assert!(
        annotations
            .iter()
            .all(|annotation| annotation.kind == SpringAnnotationKind::MethodMapping)
    );
}

#[test]
fn expands_request_mapping_methods_and_default_paths() {
    let annotations = parse_spring_route_annotation(
        "@RequestMapping(method = {RequestMethod.GET, RequestMethod.POST})",
    )
    .expect("Spring mapping");

    assert_eq!(annotations.len(), 2);
    assert_eq!(annotations[0].url, "");
    assert_eq!(annotations[0].http_method, "get");
    assert_eq!(annotations[1].http_method, "post");
    assert!(
        annotations
            .iter()
            .all(|annotation| annotation.kind == SpringAnnotationKind::RequestMapping)
    );
}

#[test]
fn parses_adjacent_route_annotations_and_returns_method_tail() {
    let (annotations, tail) = spring_route_annotations_and_tail(
        r#"@GetMapping("/users") @PostMapping("/users") public String users() {"#,
    );

    assert_eq!(annotations.len(), 2);
    assert_eq!(annotations[0].http_method, "get");
    assert_eq!(annotations[1].http_method, "post");
    assert_eq!(tail, "public String users() {");
}

#[test]
fn rejects_dynamic_concatenated_paths_without_rejecting_the_annotation() {
    let annotations = parse_spring_route_annotation(r#"@GetMapping(path = API_PREFIX + "/users")"#)
        .expect("recognized Spring mapping");

    assert!(annotations.is_empty());
}
