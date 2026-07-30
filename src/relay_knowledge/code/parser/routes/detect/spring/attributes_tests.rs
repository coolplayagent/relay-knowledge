use super::{
    extract_annotation_string_values, extract_spring_method_attributes,
    spring_annotation_uses_concatenated_path,
};

#[test]
fn extracts_positional_named_and_array_paths() {
    assert_eq!(
        extract_annotation_string_values(r#"@GetMapping("/users")"#),
        vec!["/users"]
    );
    assert_eq!(
        extract_annotation_string_values(
            r#"@GetMapping(name = "users", path = {"/users", "/members"})"#
        ),
        vec!["/users", "/members"]
    );
}

#[test]
fn rejects_concatenated_paths_without_rejecting_plus_inside_literals() {
    assert!(spring_annotation_uses_concatenated_path(
        r#"@GetMapping(path = API_PREFIX + "/users")"#
    ));
    assert!(!spring_annotation_uses_concatenated_path(
        r#"@GetMapping(path = "/users+active")"#
    ));
}

#[test]
fn extracts_supported_request_methods_and_defaults_unknown_values() {
    assert_eq!(
        extract_spring_method_attributes(
            "@RequestMapping(method = {RequestMethod.GET, POST, RequestMethod.PATCH})"
        ),
        vec!["get", "post", "patch"]
    );
    assert_eq!(
        extract_spring_method_attributes("@RequestMapping(method = CUSTOM)"),
        vec!["any"]
    );
    assert_eq!(
        extract_spring_method_attributes("@RequestMapping"),
        vec!["any"]
    );
}
