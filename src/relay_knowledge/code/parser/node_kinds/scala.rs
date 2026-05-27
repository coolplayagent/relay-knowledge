pub(super) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_definition" | "function_declaration" => Some("function"),
        "class_definition" => Some("class"),
        "trait_definition" => Some("interface"),
        "object_definition" | "package_clause" => Some("module"),
        _ => None,
    }
}

pub(super) fn is_call_node(node_kind: &str) -> bool {
    node_kind == "call_expression"
}
