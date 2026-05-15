pub(super) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_declaration" => Some("function"),
        "class_declaration" | "enum_declaration" => Some("class"),
        "object_declaration" | "package_header" => Some("module"),
        "type_alias_declaration" => Some("type"),
        _ => None,
    }
}

pub(super) fn is_call_node(node_kind: &str) -> bool {
    matches!(node_kind, "call_expression" | "new_expression")
}
