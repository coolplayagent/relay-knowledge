pub(super) fn definition_kind(node_kind: &str) -> Option<&'static str> {
    match node_kind {
        "method" | "singleton_method" => Some("method"),
        "class" => Some("class"),
        "module" => Some("module"),
        _ => None,
    }
}

pub(super) fn is_call_node(node_kind: &str) -> bool {
    node_kind == "call"
}
