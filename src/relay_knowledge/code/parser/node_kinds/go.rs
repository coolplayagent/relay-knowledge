pub(super) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_declaration" => Some("function"),
        "method_declaration" => Some("method"),
        "type_declaration" => Some("type"),
        _ => None,
    }
}

pub(super) fn is_call_node(node_kind: &str) -> bool {
    node_kind == "call_expression"
}
