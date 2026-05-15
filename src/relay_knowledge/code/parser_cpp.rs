use tree_sitter::Node;

use super::nodes::{SyntaxRange, node_text, push_children_reverse, syntax_range};

pub(super) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "function_definition" => node
            .child_by_field_name("declarator")
            .and_then(|declarator| declarator_name(content, declarator))
            .map(|name| vec![(name, "function", syntax_range(node))])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn declarator_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(
            current.kind(),
            "identifier" | "field_identifier" | "operator_name"
        ) {
            return Some(node_text(content, current));
        }
        if let Some(declarator) = current.child_by_field_name("declarator") {
            stack.push(declarator);
            continue;
        }
        push_children_reverse(current, &mut stack);
    }

    None
}
