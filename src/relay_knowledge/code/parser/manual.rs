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
        for (name, kind, range) in manual_definitions(context.content, context.language_id, node) {
            upsert_symbol(output, symbol_record(context, &name, kind, &range)?);
        }
        if let Some((name, range)) = manual_call(context, node) {
            upsert_reference(output, reference_record(context, &name, "call", &range)?);
        }
        push_children_reverse(node, &mut stack);
    }

    Ok(())
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

    generic_manual_definition(content, language_id, node)
        .into_iter()
        .collect()
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
