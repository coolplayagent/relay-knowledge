pub(in crate::code::parser) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_definition" => Some("function"),
        _ => None,
    }
}

pub(in crate::code::parser) fn is_call_node(node_kind: &str) -> bool {
    node_kind == "call"
}
