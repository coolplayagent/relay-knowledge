//! Follow bounded, preceding direct module aliases without treating reads as writes.
use crate::code::parser::nodes::node_text;
use tree_sitter::Node;

pub(super) fn refers_to(
    content: &str,
    receiver: Node<'_>,
    binding: &str,
    remaining: &mut usize,
) -> bool {
    let mut name = node_text(content, receiver);
    let mut position = receiver;
    loop {
        if name == binding {
            return true;
        }
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        let mut current = position;
        let mut replacement = None;
        'scope: while let Some(parent) = current.parent() {
            let Some(left) = remaining.checked_sub(1) else {
                return true;
            };
            *remaining = left;
            if matches!(parent.kind(), "module" | "block") {
                let mut previous = current.prev_named_sibling();
                while let Some(statement) = previous {
                    let Some(left) = remaining.checked_sub(1) else {
                        return true;
                    };
                    *remaining = left;
                    if matches!(
                        statement.kind(),
                        "if_statement"
                            | "for_statement"
                            | "while_statement"
                            | "try_statement"
                            | "with_statement"
                            | "match_statement"
                    ) && control_binds(content, statement, &name, remaining)
                    {
                        return true;
                    }
                    if statement.kind() == "expression_statement" {
                        if let Some(mut assignment) = statement
                            .named_child(0)
                            .filter(|n| n.kind() == "assignment")
                        {
                            let mut matched = false;
                            loop {
                                let Some(left) = remaining.checked_sub(1) else {
                                    return true;
                                };
                                *remaining = left;
                                matched |=
                                    assignment.child_by_field_name("left").is_some_and(|n| {
                                        n.kind() == "identifier" && node_text(content, n) == name
                                    });
                                let value = assignment
                                    .child_by_field_name("right")
                                    .and_then(|n| super::expressions::transparent(n, remaining));
                                if let Some(next) = value.filter(|n| n.kind() == "assignment") {
                                    assignment = next;
                                    continue;
                                }
                                if matched {
                                    replacement = value;
                                    break 'scope;
                                }
                                break;
                            }
                        }
                    }
                    previous = statement.prev_named_sibling();
                }
            }
            current = parent;
        }
        let Some(value) = replacement.filter(|n| n.kind() == "identifier") else {
            return false;
        };
        name = node_text(content, value);
        position = value;
    }
}

// Conditional targets may establish the receiver; unresolved control flow is
// conservative. Skip nested lexical scopes rather than treating their locals
// as assignments to this alias. Every visited syntax node consumes the budget.
fn control_binds(content: &str, node: Node<'_>, name: &str, remaining: &mut usize) -> bool {
    let deferred = super::expressions::future_annotations(content, node, remaining);
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        if matches!(
            current.kind(),
            "function_definition" | "class_definition" | "lambda"
        ) {
            continue;
        }
        let target = match current.kind() {
            "assignment" | "augmented_assignment" | "for_statement" => {
                current.child_by_field_name("left")
            }
            "named_expression" => current.child_by_field_name("name"),
            "as_pattern" => current.child_by_field_name("alias"),
            _ => None,
        };
        if let Some(target) = target {
            let mut targets = vec![target];
            while let Some(target) = targets.pop() {
                let Some(left) = remaining.checked_sub(1) else {
                    return true;
                };
                *remaining = left;
                if target.kind() == "identifier" && node_text(content, target) == name {
                    return true;
                }
                if matches!(target.kind(), "attribute" | "subscript") {
                    continue;
                }
                let mut cursor = target.walk();
                targets.extend(target.named_children(&mut cursor));
            }
        }
        if !super::expressions::eager_children(current, &mut stack, remaining, deferred) {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "aliases_tests.rs"]
mod tests;
