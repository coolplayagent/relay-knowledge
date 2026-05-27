pub(super) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "method_declaration" | "method_signature" => Some("method"),
        "class_declaration" | "enum_declaration" => Some("class"),
        "interface_declaration" => Some("interface"),
        _ => None,
    }
}

pub(super) fn is_call_node(node_kind: &str) -> bool {
    matches!(
        node_kind,
        "call_expression" | "new_expression" | "object_creation_expression"
    )
}
