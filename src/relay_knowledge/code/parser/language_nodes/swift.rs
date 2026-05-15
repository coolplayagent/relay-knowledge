pub(super) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_declaration" => Some("function"),
        "class_declaration" | "enum_declaration" | "struct_declaration" => Some("class"),
        "protocol_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type"),
        _ => None,
    }
}

pub(super) fn is_call_node(node_kind: &str) -> bool {
    matches!(node_kind, "call_expression" | "new_expression")
}
