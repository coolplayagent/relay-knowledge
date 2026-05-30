use tree_sitter::Node;

use crate::code::configuration::{self, ConfigFact, ConfigReference};

use super::{
    super::CodeIndexError,
    FileParseContext, FileParseOutput, languages,
    nodes::{SyntaxRange, last_identifier_text, node_text, push_children_reverse, syntax_range},
    records::{reference_record, symbol_record, upsert_reference, upsert_symbol},
};

pub(super) fn collect_manual_nodes(
    context: &FileParseContext<'_>,
    root: Node<'_>,
    config_definitions: &[ConfigFact],
    config_references: &[ConfigReference],
    output: &mut FileParseOutput,
) -> Result<(), CodeIndexError> {
    let mut stack = Vec::with_capacity(root.child_count().saturating_add(1));
    stack.push(root);
    while let Some(node) = stack.pop() {
        if languages::manual_definition_candidate(context.language_id, node.kind()) {
            for (name, kind, range) in
                manual_definitions(context.content, context.language_id, node)
            {
                upsert_symbol(output, symbol_record(context, &name, kind, &range)?);
            }
        }
        if let Some((name, kind, range)) =
            enum_member_definition(context.content, context.language_id, node)
        {
            upsert_symbol(output, symbol_record(context, &name, kind, &range)?);
        }
        if let Some((name, range)) = manual_call(context, node) {
            upsert_reference(output, reference_record(context, &name, "call", &range)?);
        }
        if let Some((name, kind, range)) = manual_reference(context, node) {
            upsert_reference(output, reference_record(context, &name, kind, &range)?);
        }
        push_children_reverse(node, &mut stack);
    }
    for (name, kind, range) in
        languages::language_manual_file_definitions(context.content, context.language_id)
    {
        upsert_symbol(output, symbol_record(context, &name, kind, &range)?);
    }
    for definition in config_definitions {
        upsert_symbol(
            output,
            symbol_record(
                context,
                &definition.name,
                definition.kind,
                &definition.range.into(),
            )?,
        );
    }
    for reference in config_references {
        upsert_reference(
            output,
            reference_record(
                context,
                &reference.name,
                reference.kind,
                &reference.range.into(),
            )?,
        );
    }

    Ok(())
}

impl From<configuration::ConfigRange> for SyntaxRange {
    fn from(range: configuration::ConfigRange) -> Self {
        Self {
            byte_start: range.byte_start,
            byte_end: range.byte_end,
            line_start: range.line_start,
            line_end: range.line_end,
        }
    }
}

pub(super) fn manual_definitions(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let definitions = languages::language_manual_definitions(content, language_id, node);
    if !definitions.is_empty() {
        return definitions;
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
    if languages::generic_manual_definition_is_rejected(content, language_id, node) {
        return None;
    }
    let kind = languages::definition_kind(language_id, node.kind())?;
    let name = node.child_by_field_name("name")?;
    let range = languages::language_exported_declaration_range(language_id, node)
        .unwrap_or_else(|| syntax_range(node));

    Some((node_text(content, name), kind, range))
}

fn enum_member_definition(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    languages::language_enum_member_definition(content, language_id, node)
}

fn manual_call(context: &FileParseContext<'_>, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    if node.kind() == "preproc_call" {
        let name = node
            .child_by_field_name("directive")
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| super::nodes::first_named_child_of_kind(node, "identifier"))?;
        return Some((node_text(context.content, name), syntax_range(name)));
    }
    if let Some(call) = languages::language_manual_call(context.content, context.language_id, node)
    {
        return Some(call);
    }
    if !languages::is_call_node(context.language_id, node.kind()) {
        return None;
    }
    if let Some(call) = constructed_type_call(context.content, node) {
        return Some(call);
    }
    let function = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"))
        .or_else(|| node.child(0))?;

    callable_expression_name(context.content, function)
}

fn callable_expression_name(content: &str, function: Node<'_>) -> Option<(String, SyntaxRange)> {
    if function.kind() == "subscript_expression" {
        let argument = function.child_by_field_name("argument");
        for index in 0..function.named_child_count() {
            let child = function.named_child(u32::try_from(index).ok()?)?;
            if argument.is_some_and(|argument| node_contains(argument, child)) {
                continue;
            }
            if let Some(callable) = callable_expression_name(content, child) {
                return Some(callable);
            }
        }
    }

    last_identifier_text(content, function).map(|name| (name, syntax_range(function)))
}

fn manual_reference(
    context: &FileParseContext<'_>,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    languages::language_manual_reference(context.content, context.language_id, node)
}

fn node_contains(parent: Node<'_>, child: Node<'_>) -> bool {
    parent.start_byte() <= child.start_byte() && parent.end_byte() >= child.end_byte()
}

fn constructed_type_call(content: &str, node: Node<'_>) -> Option<(String, SyntaxRange)> {
    let type_node = node.child_by_field_name("type")?;
    let name = constructed_type_name(content, type_node)?;

    Some((name, syntax_range(type_node)))
}

fn constructed_type_name(content: &str, node: Node<'_>) -> Option<String> {
    match node.kind() {
        "generic_type" => first_constructed_type_child(node)
            .and_then(|inner| constructed_type_name(content, inner)),
        "array_type" => node
            .child_by_field_name("element")
            .and_then(|inner| constructed_type_name(content, inner)),
        "scoped_type_identifier" => last_type_identifier_text(content, node),
        "identifier" | "type_identifier" => Some(node_text(content, node)),
        _ => last_type_identifier_text(content, node),
    }
}

fn first_constructed_type_child(node: Node<'_>) -> Option<Node<'_>> {
    (0..node.child_count()).find_map(|index| {
        let index = u32::try_from(index).ok()?;
        let child = node.child(index)?;
        constructed_type_node(child.kind()).then_some(child)
    })
}

fn last_type_identifier_text(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = Vec::with_capacity(node.child_count().saturating_add(1));
    stack.push(node);
    let mut last = None;
    while let Some(current) = stack.pop() {
        if current.kind() == "type_arguments" {
            continue;
        }
        if matches!(current.kind(), "identifier" | "type_identifier") {
            last = Some(node_text(content, current));
            continue;
        }
        push_children_reverse(current, &mut stack);
    }

    last
}

fn constructed_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "array_type" | "generic_type" | "identifier" | "scoped_type_identifier" | "type_identifier"
    )
}
