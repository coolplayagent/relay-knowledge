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

/// Schedule only expressions evaluated when this expression is created.
pub(super) fn eager_children<'a>(
    node: Node<'a>,
    stack: &mut Vec<Node<'a>>,
    remaining: &mut usize,
) -> bool {
    let selected = match node.kind() {
        "lambda" => Some(node.child_by_field_name("parameters")),
        "generator_expression" => {
            let mut cursor = node.walk();
            let mut iterable = None;
            for child in node.named_children(&mut cursor) {
                let Some(left) = remaining.checked_sub(1) else {
                    return false;
                };
                *remaining = left;
                if child.kind() == "for_in_clause" {
                    iterable = child.child_by_field_name("right");
                    break;
                }
            }
            Some(iterable)
        }
        _ => None,
    };
    if let Some(selected) = selected {
        if let Some(child) = selected {
            let Some(left) = remaining.checked_sub(1) else {
                return false;
            };
            *remaining = left;
            stack.push(child);
        }
        return true;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        stack.push(child);
    }
    true
}

pub(super) fn annotation_binds_local(mut node: Node<'_>, remaining: &mut usize) -> bool {
    while let Some(parent) = node.parent() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        match parent.kind() {
            "function_definition" | "lambda" => return true,
            "class_definition" | "module" => return false,
            _ => node = parent,
        }
    }
    false
}

pub(super) fn has_eager_call(statement: Node<'_>, remaining: &mut usize) -> bool {
    if statement.kind() != "expression_statement" {
        return false;
    }
    let mut stack = vec![statement];
    while let Some(node) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        if node.kind() == "call" {
            return true;
        }
        if !eager_children(node, &mut stack, remaining) {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "expressions_tests.rs"]
mod tests;
