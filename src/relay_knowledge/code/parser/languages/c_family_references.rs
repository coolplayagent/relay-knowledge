use tree_sitter::Node;

use super::super::nodes::{SyntaxRange, node_text, syntax_range};

pub(in crate::code::parser) fn manual_reference(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !c_family_reference_node(node.kind()) {
        return None;
    }
    let name = node_text(content, node);
    if !c_family_reference_name(&name) {
        return None;
    }
    if node.kind() == "type_identifier" && c_family_type_reference_context(node) {
        return Some((name, "type", syntax_range(node)));
    }
    if c_family_value_reference_context(node) {
        return Some((name, "implementation", syntax_range(node)));
    }

    None
}

fn c_family_reference_node(kind: &str) -> bool {
    matches!(
        kind,
        "identifier" | "field_identifier" | "namespace_identifier" | "type_identifier"
    )
}

fn c_family_reference_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn c_family_type_reference_context(node: Node<'_>) -> bool {
    has_ancestor_kind(node, "field_declaration")
        || has_ancestor_kind(node, "parameter_declaration")
        || has_ancestor_kind(node, "qualified_type_identifier")
        || has_ancestor_kind(node, "scoped_type_identifier")
}

fn c_family_value_reference_context(node: Node<'_>) -> bool {
    has_ancestor_kind(node, "initializer_list")
        || has_non_argument_subscript_ancestor(node)
        || has_ancestor_kind(node, "qualified_identifier")
        || has_ancestor_kind(node, "scoped_identifier")
}

fn has_non_argument_subscript_ancestor(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == "subscript_expression" {
            return !parent
                .child_by_field_name("argument")
                .is_some_and(|argument| node_contains(argument, node));
        }
        node = parent;
    }

    false
}

fn has_ancestor_kind(mut node: Node<'_>, kind: &str) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return true;
        }
        node = parent;
    }

    false
}

fn node_contains(parent: Node<'_>, child: Node<'_>) -> bool {
    parent.start_byte() <= child.start_byte() && parent.end_byte() >= child.end_byte()
}
