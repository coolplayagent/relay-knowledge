use super::*;

#[test]
fn decorated_head_name_scan_finds_the_top_level_function_declarator() {
    assert_eq!(
        c_function_name_from_decorated_head("static int handle_request(int value)"),
        Some("handle_request".to_owned())
    );
    assert!(c_decorated_function_head_is_declaration(
        "static int handle_request(int value)",
        "handle_request"
    ));
}

#[test]
fn decorated_head_name_scan_rejects_missing_declarators_and_mismatched_names() {
    assert_eq!(
        c_function_name_from_decorated_head("static int handle_request"),
        None
    );
    assert!(!c_decorated_function_head_is_declaration(
        "static int handle_request(int value)",
        "other"
    ));
}
