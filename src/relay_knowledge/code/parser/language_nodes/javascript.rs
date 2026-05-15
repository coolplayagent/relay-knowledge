pub(super) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_declaration" => Some("function"),
        "method_definition" => Some("method"),
        "class_declaration" => Some("class"),
        _ => None,
    }
}

pub(super) fn is_call_node(node_kind: &str) -> bool {
    matches!(node_kind, "call_expression" | "new_expression")
}
