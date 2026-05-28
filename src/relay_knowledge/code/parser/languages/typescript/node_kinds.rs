pub(in crate::code::parser) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_declaration" => Some("function"),
        "method_definition" | "method_signature" => Some("method"),
        "class_declaration" | "enum_declaration" => Some("class"),
        "interface_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type"),
        _ => None,
    }
}

pub(in crate::code::parser) fn is_call_node(node_kind: &str) -> bool {
    matches!(node_kind, "call_expression" | "new_expression")
}
