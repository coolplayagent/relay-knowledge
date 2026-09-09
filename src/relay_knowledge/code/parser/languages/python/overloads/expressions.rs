//! Transparent expression syntax shared by decorator and mutation proofs.
use tree_sitter::Node;

pub(super) fn transparent<'a>(mut node: Node<'a>, remaining: &mut usize) -> Option<Node<'a>> {
    loop {
        *remaining = remaining.checked_sub(1)?;
        if node.kind() != "parenthesized_expression" {
            return Some(node);
        }
        let mut expression = None;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            *remaining = remaining.checked_sub(1)?;
            if child.kind() == "comment" {
                continue;
            }
            if expression.replace(child).is_some() {
                return None;
            }
        }
        node = expression?;
    }
}

#[cfg(test)]
#[path = "expressions_tests.rs"]
mod tests;
