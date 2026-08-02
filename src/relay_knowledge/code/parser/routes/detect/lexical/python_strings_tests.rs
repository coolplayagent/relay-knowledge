//! Unit contract for static Python route string parsing.

use super::extract_quoted_string_python;

#[test]
fn extracts_plain_and_prefixed_python_strings() {
    assert_eq!(
        extract_quoted_string_python(" '/users'"),
        Some("/users".to_owned())
    );
    assert_eq!(
        extract_quoted_string_python("r\"/users/<id>\""),
        Some("/users/<id>".to_owned())
    );
    assert_eq!(
        extract_quoted_string_python("Br'/health'"),
        Some("/health".to_owned())
    );
}

#[test]
fn rejects_dynamic_python_expressions() {
    assert_eq!(extract_quoted_string_python("f'/users/{user_id}'"), None);
    assert_eq!(extract_quoted_string_python("route_name"), None);
}

#[test]
fn unescapes_quoted_route_content() {
    assert_eq!(
        extract_quoted_string_python(r#""/path\/sub""#),
        Some("/path/sub".to_owned())
    );
}
