//! Unit contract for C++ tree-sitter node classification.

use super::*;

#[test]
fn definition_kinds_cover_cpp_declaration_families() {
    let cases = [
        ("function_definition", Some("function")),
        ("function_declaration", Some("function")),
        ("method_definition", Some("method")),
        ("class_specifier", Some("class")),
        ("enum_specifier", Some("class")),
        ("struct_specifier", Some("class")),
        ("namespace_definition", Some("module")),
        ("type_definition", Some("type")),
        ("declaration", None),
    ];

    for (node_kind, expected) in cases {
        assert_eq!(
            definition_kind(node_kind),
            expected,
            "node kind {node_kind}"
        );
    }
}

#[test]
fn only_call_expressions_are_call_nodes() {
    assert!(is_call_node("call_expression"));
    assert!(!is_call_node("function_definition"));
    assert!(!is_call_node("new_expression"));
}
