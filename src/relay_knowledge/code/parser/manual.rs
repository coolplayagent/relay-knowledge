use tree_sitter::Node;

use super::{
    super::CodeIndexError,
    FileParseContext, FileParseOutput, language_nodes,
    nodes::{SyntaxRange, last_identifier_text, node_text, push_children_reverse, syntax_range},
    records::{reference_record, symbol_record, upsert_reference, upsert_symbol},
};

pub(super) fn collect_manual_nodes(
    context: &FileParseContext<'_>,
    root: Node<'_>,
    output: &mut FileParseOutput,
) -> Result<(), CodeIndexError> {
    let mut stack = Vec::with_capacity(root.child_count().saturating_add(1));
    stack.push(root);
    while let Some(node) = stack.pop() {
        if manual_definition_candidate(context.language_id, node.kind()) {
            for (name, kind, range) in
                manual_definitions(context.content, context.language_id, node)
            {
                upsert_symbol(output, symbol_record(context, &name, kind, &range)?);
            }
        }
        if let Some((name, range)) = manual_call(context, node) {
            upsert_reference(output, reference_record(context, &name, "call", &range)?);
        }
        push_children_reverse(node, &mut stack);
    }

    Ok(())
}

fn manual_definition_candidate(language_id: &str, node_kind: &str) -> bool {
    match language_id {
        "c" => {
            matches!(
                node_kind,
                "declaration" | "preproc_def" | "preproc_function_def" | "call_expression"
            ) || language_nodes::definition_kind(language_id, node_kind).is_some()
        }
        "cpp" => {
            node_kind == "function_definition"
                || language_nodes::definition_kind(language_id, node_kind).is_some()
        }
        "javascript" | "jsx" | "typescript" | "tsx" => {
            matches!(node_kind, "variable_declarator" | "public_field_definition")
                || language_nodes::definition_kind(language_id, node_kind).is_some()
        }
        _ => language_nodes::definition_kind(language_id, node_kind).is_some(),
    }
}

pub(super) fn manual_definitions(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    if language_id == "c" {
        let definitions = super::parser_c::manual_definitions(content, node);
        if !definitions.is_empty() {
            return definitions;
        }
    }
    if language_id == "cpp" {
        let definitions = super::parser_cpp::manual_definitions(content, node);
        if !definitions.is_empty() {
            return definitions;
        }
    }
    if let Some(definition) = javascript_like_function_value_definition(content, language_id, node)
    {
        return vec![definition];
    }

    generic_manual_definition(content, language_id, node)
        .into_iter()
        .collect()
}

fn javascript_like_function_value_definition(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
        || !matches!(
            node.kind(),
            "variable_declarator" | "public_field_definition"
        )
    {
        return None;
    }
    let value = node.child_by_field_name("value")?;
    if !matches!(value.kind(), "arrow_function" | "function_expression") {
        return None;
    }
    let name = node.child_by_field_name("name")?;
    let name = node_text(content, name);
    if !javascript_identifier_name(&name) {
        return None;
    }

    Some((name, "function", syntax_range(node)))
}

fn javascript_identifier_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|character| {
        character == '_' || character == '$' || character.is_ascii_alphabetic()
    }) && characters
        .all(|character| character == '_' || character == '$' || character.is_ascii_alphanumeric())
}

fn generic_manual_definition(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let kind = language_nodes::definition_kind(language_id, node.kind())?;
    let name = node.child_by_field_name("name")?;

    Some((node_text(content, name), kind, syntax_range(node)))
}

fn manual_call(context: &FileParseContext<'_>, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    if node.kind() == "preproc_call" {
        let name = node
            .child_by_field_name("directive")
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| super::nodes::first_named_child_of_kind(node, "identifier"))?;
        return Some((node_text(context.content, name), syntax_range(name)));
    }
    if !language_nodes::is_call_node(context.language_id, node.kind()) {
        return None;
    }
    let function = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"))
        .or_else(|| node.child(0))?;

    last_identifier_text(context.content, function).map(|name| (name, syntax_range(function)))
}
