pub(super) mod bash;
pub(super) mod c;
mod c_family_references;
mod config;
pub(super) mod cpp;
pub(super) mod csharp;
mod enum_members;
pub(super) mod go;
pub(super) mod java;
pub(super) mod javascript;
pub(super) mod kotlin;
pub(super) mod php;
pub(super) mod python;
pub(super) mod ruby;
pub(super) mod rust;
pub(super) mod scala;
pub(super) mod swift;
pub(super) mod typescript;

use tree_sitter::Node;

use super::nodes::SyntaxRange;

pub(super) fn definition_kind(language_id: &str, node_kind: &str) -> Option<&'static str> {
    match language_id {
        "bash" => bash::definition_kind(node_kind),
        "c" => c::definition_kind(node_kind),
        "cpp" => cpp::definition_kind(node_kind),
        "csharp" => csharp::definition_kind(node_kind),
        "go" => go::definition_kind(node_kind),
        "java" => java::definition_kind(node_kind),
        "javascript" | "jsx" => javascript::definition_kind(node_kind),
        "ini" | "json" | "properties" | "toml" | "yaml" => {
            config::definition_kind(language_id, node_kind)
        }
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
        "ini" | "json" | "properties" | "toml" | "yaml" => false,
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

pub(super) fn javascript_like_dynamic_import(
    language_id: &str,
    content: &str,
    node: Node<'_>,
) -> Option<(String, SyntaxRange)> {
    javascript::dynamic_import(language_id, content, node)
}

pub(super) fn javascript_like_re_export(
    language_id: &str,
    content: &str,
    node: Node<'_>,
) -> Option<(String, SyntaxRange)> {
    javascript::re_export(language_id, content, node)
}

pub(super) fn manual_definition_candidate(language_id: &str, node_kind: &str) -> bool {
    match language_id {
        "c" => {
            matches!(
                node_kind,
                "declaration"
                    | "ERROR"
                    | "preproc_def"
                    | "preproc_function_def"
                    | "call_expression"
            ) || definition_kind(language_id, node_kind).is_some()
        }
        "cpp" => {
            matches!(
                node_kind,
                "class_specifier"
                    | "declaration"
                    | "enum_specifier"
                    | "ERROR"
                    | "function_definition"
                    | "struct_specifier"
                    | "union_specifier"
            ) || definition_kind(language_id, node_kind).is_some()
        }
        "javascript" | "jsx" | "typescript" | "tsx" => {
            javascript::manual_definition_candidate(node_kind)
                || definition_kind(language_id, node_kind).is_some()
        }
        "ini" | "json" | "properties" | "toml" | "yaml" => {
            config::manual_definition_candidate(language_id, node_kind)
        }
        _ => definition_kind(language_id, node_kind).is_some(),
    }
}

pub(super) fn language_manual_definitions(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match language_id {
        "c" => c::manual_definitions(content, node),
        "cpp" => cpp::manual_definitions(content, node),
        "ini" | "json" | "properties" | "toml" | "yaml" => {
            config::manual_definitions(content, language_id, node)
        }
        "javascript" | "jsx" | "typescript" | "tsx" => javascript::manual_definition(content, node)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn language_exported_declaration_range(
    language_id: &str,
    node: Node<'_>,
) -> Option<SyntaxRange> {
    match language_id {
        "javascript" | "jsx" | "typescript" | "tsx" => javascript::exported_declaration_range(node),
        _ => None,
    }
}

pub(super) fn language_manual_reference(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    match language_id {
        "c" | "cpp" => c_family_references::manual_reference(content, node),
        _ => None,
    }
}

pub(super) fn language_enum_member_definition(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    enum_members::enum_member_definition(content, language_id, node)
}

pub(super) fn generic_manual_definition_is_rejected(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> bool {
    matches!(language_id, "cpp")
        && node.kind() == "function_definition"
        && cpp::function_definition_is_destructor(content, node)
}
