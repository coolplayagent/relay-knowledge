mod node_kinds;

use tree_sitter::Node;

use super::super::nodes::{SyntaxRange, last_identifier_text, syntax_range};

pub(in crate::code::parser) use node_kinds::{definition_kind, is_call_node};

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let Some(trait_node) = node.child_by_field_name("trait") else {
        return Vec::new();
    };
    let Some(target_node) = node.child_by_field_name("type") else {
        return Vec::new();
    };
    let target_identity_node = target_node
        .child_by_field_name("type")
        .unwrap_or(target_node);
    let Some(target_name) = last_identifier_text(content, target_identity_node) else {
        return Vec::new();
    };
    if last_identifier_text(content, trait_node).is_none() {
        return Vec::new();
    }

    vec![(target_name, "implementation", syntax_range(node))]
}
