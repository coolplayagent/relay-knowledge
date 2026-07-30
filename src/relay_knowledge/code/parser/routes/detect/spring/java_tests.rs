use super::{
    java_code_lines_without_comments, line_declares_java_type,
    line_declares_nested_java_helper_type, parse_java_method_def, update_java_brace_depth,
};

#[test]
fn code_lines_remove_comments_and_text_blocks_but_preserve_quoted_markers() {
    let source = r#"
        String endpoint = "https://service/users"; // trailing comment
        /*
         * @GetMapping("/commented")
         */
        String template = """
            @GetMapping("/text-block")
        """;
        @GetMapping("/active")
    "#;

    let code = java_code_lines_without_comments(source).join("\n");

    assert!(code.contains(r#"String endpoint = "https://service/users";"#));
    assert!(code.contains(r#"@GetMapping("/active")"#));
    assert!(!code.contains("/commented"));
    assert!(!code.contains("/text-block"));
}

#[test]
fn brace_depth_ignores_braces_inside_literals() {
    let mut depth = 0;

    update_java_brace_depth(r#"class Controller { String close = "}";"#, &mut depth);
    assert_eq!(depth, 1);

    update_java_brace_depth("}", &mut depth);
    assert_eq!(depth, 0);
}

#[test]
fn declaration_helpers_distinguish_types_methods_and_annotations() {
    assert!(line_declares_java_type("public class UserController {"));
    assert!(line_declares_nested_java_helper_type(
        "private static class Mapper {",
        1
    ));
    assert!(!line_declares_nested_java_helper_type(
        "private static class Mapper {",
        0
    ));
    assert_eq!(
        parse_java_method_def("public <T> T listUsers(Request request) {"),
        Some("listUsers".to_owned())
    );
    assert_eq!(parse_java_method_def("@GetMapping(\"/users\")"), None);
}
