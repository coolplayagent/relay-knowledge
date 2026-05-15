mod bash;
mod c;
mod cpp;
mod csharp;
mod go;
mod java;
mod javascript;
mod kotlin;
mod php;
mod python;
mod ruby;
mod rust;
mod scala;
mod swift;
mod typescript;

pub(super) fn definition_kind(language_id: &str, node_kind: &str) -> Option<&'static str> {
    match language_id {
        "bash" => bash::definition_kind(node_kind),
        "c" => c::definition_kind(node_kind),
        "cpp" => cpp::definition_kind(node_kind),
        "csharp" => csharp::definition_kind(node_kind),
        "go" => go::definition_kind(node_kind),
        "java" => java::definition_kind(node_kind),
        "javascript" | "jsx" => javascript::definition_kind(node_kind),
        "kotlin" => kotlin::definition_kind(node_kind),
        "php" => php::definition_kind(node_kind),
        "python" => python::definition_kind(node_kind),
        "ruby" => ruby::definition_kind(node_kind),
        "rust" => rust::definition_kind(node_kind),
        "scala" => scala::definition_kind(node_kind),
        "swift" => swift::definition_kind(node_kind),
        "typescript" | "tsx" => typescript::definition_kind(node_kind),
        _ => None,
    }
}

pub(super) fn is_call_node(language_id: &str, node_kind: &str) -> bool {
    match language_id {
        "bash" => bash::is_call_node(node_kind),
        "c" => c::is_call_node(node_kind),
        "cpp" => cpp::is_call_node(node_kind),
        "csharp" => csharp::is_call_node(node_kind),
        "go" => go::is_call_node(node_kind),
        "java" => java::is_call_node(node_kind),
        "javascript" | "jsx" => javascript::is_call_node(node_kind),
        "kotlin" => kotlin::is_call_node(node_kind),
        "php" => php::is_call_node(node_kind),
        "python" => python::is_call_node(node_kind),
        "ruby" => ruby::is_call_node(node_kind),
        "rust" => rust::is_call_node(node_kind),
        "scala" => scala::is_call_node(node_kind),
        "swift" => swift::is_call_node(node_kind),
        "typescript" | "tsx" => typescript::is_call_node(node_kind),
        _ => false,
    }
}
