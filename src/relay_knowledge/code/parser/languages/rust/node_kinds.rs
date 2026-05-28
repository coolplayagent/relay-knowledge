pub(in crate::code::parser) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_item" => Some("function"),
        "struct_item" | "enum_item" => Some("class"),
        "trait_item" => Some("interface"),
        "type_item" => Some("type"),
        _ => None,
    }
}

pub(in crate::code::parser) fn is_call_node(node_kind: &str) -> bool {
    node_kind == "call_expression"
}
