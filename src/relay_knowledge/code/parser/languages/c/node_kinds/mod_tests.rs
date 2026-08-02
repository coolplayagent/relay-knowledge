use super::*;

#[test]
fn c_node_kinds_distinguish_definitions_calls_and_unrelated_nodes() {
    assert_eq!(definition_kind("function_definition"), Some("function"));
    assert_eq!(definition_kind("function_declaration"), Some("function"));
    assert_eq!(definition_kind("type_definition"), Some("type"));
    assert_eq!(definition_kind("declaration"), None);
    assert!(is_call_node("call_expression"));
    assert!(!is_call_node("function_declarator"));
}
