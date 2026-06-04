pub(in crate::code::parser) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "method_declaration" => Some("method"),
        "class_declaration" | "enum_declaration" | "struct_declaration" => Some("class"),
        "interface_declaration" => Some("interface"),
        "namespace_declaration" => Some("module"),
        _ => None,
    }
}

pub(in crate::code::parser) fn is_call_node(node_kind: &str) -> bool {
    matches!(
        node_kind,
        "call_expression"
            | "invocation_expression"
            | "new_expression"
            | "object_creation_expression"
    )
}
