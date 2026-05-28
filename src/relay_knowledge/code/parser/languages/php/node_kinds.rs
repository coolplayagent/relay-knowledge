pub(in crate::code::parser) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_definition" => Some("function"),
        "method_declaration" => Some("method"),
        "class_declaration" | "enum_declaration" => Some("class"),
        "interface_declaration" | "trait_declaration" => Some("interface"),
        "namespace_definition" => Some("module"),
        _ => None,
    }
}

pub(in crate::code::parser) fn is_call_node(node_kind: &str) -> bool {
    matches!(node_kind, "call_expression" | "new_expression")
}
