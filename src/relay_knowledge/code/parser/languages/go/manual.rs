use tree_sitter::Node;

use crate::code::parser::nodes::{SyntaxRange, node_text, push_children_reverse, syntax_range};

pub(in crate::code::parser) fn manual_definition_candidate(node_kind: &str) -> bool {
    matches!(node_kind, "type_declaration" | "type_spec")
}

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "type_declaration" => type_declaration_definitions(content, node),
        "type_spec" if !has_parent_kind(node, "type_declaration") => {
            type_spec_definition(content, node, None)
                .into_iter()
                .collect()
        }
        _ => Vec::new(),
    }
}

fn type_declaration_definitions(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let mut definitions = Vec::new();
    let mut stack = Vec::with_capacity(declaration.child_count().saturating_add(1));
    push_children_reverse(declaration, &mut stack);
    while let Some(node) = stack.pop() {
        if node.kind() == "type_spec" {
            if let Some(definition) = type_spec_definition(
                content,
                node,
                single_type_declaration_range(content, declaration, node),
            ) {
                definitions.push(definition);
            }
            continue;
        }
        push_children_reverse(node, &mut stack);
    }

    definitions
}

fn type_spec_definition(
    content: &str,
    spec: Node<'_>,
    range: Option<SyntaxRange>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let name = spec.child_by_field_name("name")?;
    let name = node_text(content, name);
    if !go_identifier(&name) {
        return None;
    }
    let kind = spec
        .child_by_field_name("type")
        .map(|type_node| go_type_spec_kind(type_node.kind()))
        .unwrap_or("type");

    Some((name, kind, range.unwrap_or_else(|| syntax_range(spec))))
}

fn single_type_declaration_range(
    content: &str,
    declaration: Node<'_>,
    spec: Node<'_>,
) -> Option<SyntaxRange> {
    let declaration_text = node_text(content, declaration);
    let trimmed = declaration_text.trim_start();
    (trimmed.starts_with("type ") && !trimmed.starts_with("type (")).then(|| {
        let declaration_range = syntax_range(declaration);
        SyntaxRange {
            byte_start: declaration_range.byte_start,
            byte_end: spec.end_byte(),
            line_start: declaration_range.line_start,
            line_end: spec.end_position().row + 1,
        }
    })
}

fn go_type_spec_kind(type_kind: &str) -> &'static str {
    match type_kind {
        "interface_type" => "interface",
        "struct_type" => "struct",
        _ => "type",
    }
}

fn has_parent_kind(node: Node<'_>, kind: &str) -> bool {
    node.parent().is_some_and(|parent| parent.kind() == kind)
}

fn go_identifier(name: &str) -> bool {
    if name == "_" {
        return false;
    }
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}
