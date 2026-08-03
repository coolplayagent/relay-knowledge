//! C-family manual reference recognition and context classification.

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
    let context = c_family_reference_context(node);
    if node.kind() == "type_identifier" && context.type_reference {
        return Some((name, "type", syntax_range(node)));
    }
    if context.value_reference {
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

#[derive(Default)]
struct CFamilyReferenceContext {
    type_reference: bool,
    value_reference: bool,
}

fn c_family_reference_context(node: Node<'_>) -> CFamilyReferenceContext {
    let mut context = CFamilyReferenceContext::default();
    let mut subscript_reference = None;
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "field_declaration"
            | "parameter_declaration"
            | "qualified_type_identifier"
            | "scoped_type_identifier" => context.type_reference = true,
            "initializer_list" | "qualified_identifier" | "scoped_identifier" => {
                context.value_reference = true;
            }
            "subscript_expression" if subscript_reference.is_none() => {
                subscript_reference = Some(
                    !parent
                        .child_by_field_name("argument")
                        .is_some_and(|argument| node_contains(argument, node)),
                );
            }
            _ => {}
        }
        if context.type_reference && context.value_reference {
            break;
        }
        current = parent;
    }
    context.value_reference |= subscript_reference.unwrap_or(false);

    context
}

fn node_contains(parent: Node<'_>, child: Node<'_>) -> bool {
    parent.start_byte() <= child.start_byte() && parent.end_byte() >= child.end_byte()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
