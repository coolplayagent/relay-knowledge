pub(super) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "function_definition" | "function_declaration" => Some("function"),
        "method_definition" => Some("method"),
        "class_specifier" | "enum_specifier" | "struct_specifier" => Some("class"),
        "namespace_definition" => Some("module"),
        "type_definition" => Some("type"),
        _ => None,
    }
}

pub(super) fn is_call_node(node_kind: &str) -> bool {
    node_kind == "call_expression"
}
