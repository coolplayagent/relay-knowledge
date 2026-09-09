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

#[cfg(test)]
#[path = "aliases_tests.rs"]
mod tests;
