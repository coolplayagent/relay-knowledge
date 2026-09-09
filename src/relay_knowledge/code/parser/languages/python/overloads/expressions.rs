//! Transparent expression syntax shared by decorator and mutation proofs.
use crate::code::parser::nodes::node_text;
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
    deferred_annotations: bool,
) -> bool {
    let selected = match node.kind() {
        "class_definition" => Some(node.child_by_field_name("superclasses")),
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
        if node.kind() == "function_definition"
            && child == node.child_by_field_name("body").unwrap_or(node)
        {
            continue;
        }
        let annotation = node.child_by_field_name("type") == Some(child)
            || node.child_by_field_name("return_type") == Some(child);
        if annotation
            && (deferred_annotations
                || (node.kind() == "assignment" && annotation_binds_local(node, remaining)))
        {
            continue;
        }
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

pub(super) fn has_eager_call(content: &str, statement: Node<'_>, remaining: &mut usize) -> bool {
    let deferred = future_annotations(content, statement, remaining);
    let mut stack = vec![statement];
    while let Some(node) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        if node.kind() == "call" {
            return true;
        }
        if !eager_children(node, &mut stack, remaining, deferred) {
            return true;
        }
    }
    false
}

/// Only an explicit future import changes the annotation evaluation contract.
pub(super) fn future_annotations(content: &str, mut node: Node<'_>, remaining: &mut usize) -> bool {
    while let Some(parent) = node.parent() {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        node = parent;
    }
    let mut initial = true;
    let mut cursor = node.walk();
    for statement in node.named_children(&mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        if statement.kind() == "comment" {
            continue;
        }
        if initial
            && statement.kind() == "expression_statement"
            && statement
                .named_child(0)
                .is_some_and(|n| n.kind() == "string")
        {
            initial = false;
            continue;
        }
        initial = false;
        // Python only permits future imports before executable statements and
        // ordinary imports. A later spelling is not an active future feature.
        if statement.kind() != "future_import_statement" {
            return false;
        }
        let mut names = statement.walk();
        for name in statement.named_children(&mut names) {
            let Some(left) = remaining.checked_sub(1) else {
                return false;
            };
            *remaining = left;
            if node_text(content, name) == "annotations" {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
#[path = "expressions_tests.rs"]
mod tests;
