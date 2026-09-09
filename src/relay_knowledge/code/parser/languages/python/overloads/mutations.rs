//! Bounded mutation targets affecting a proven typing module's overload value.
use crate::code::parser::nodes::node_text;
use tree_sitter::Node;
const MAX_BINDING_STATEMENTS: usize = 1024;
pub(super) fn expression_rebinds(
    content: &str,
    mut node: Node<'_>,
    name: &str,
    module: bool,
) -> bool {
    if !matches!(node.kind(), "assignment" | "augmented_assignment") {
        return false;
    }
    let mut remaining = MAX_BINDING_STATEMENTS;
    loop {
        if remaining == 0 {
            return true;
        }
        remaining -= 1;
        let Some(left) = node.child_by_field_name("left") else {
            return false;
        };
        if assignment_binds(content, left, name, module, &mut remaining) {
            return true;
        }
        // Chained assignments nest on the right; ordinary RHS reads are not targets.
        let Some(right) = node
            .child_by_field_name("right")
            .filter(|right| right.kind() == "assignment")
        else {
            return false;
        };
        node = right;
    }
}

fn assignment_binds(
    content: &str,
    node: Node<'_>,
    name: &str,
    module: bool,
    remaining: &mut usize,
) -> bool {
    let mut cursor = node.walk();
    loop {
        if *remaining == 0 {
            return true;
        }
        *remaining -= 1;
        let current = cursor.node();
        if current.kind() == "identifier" && node_text(content, current) == name {
            return true;
        }
        if module
            && matches!(current.kind(), "attribute" | "subscript")
            && module_receiver(content, current, name, remaining)
        {
            return true;
        }
        if !matches!(current.kind(), "attribute" | "subscript") && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return false;
            }
        }
    }
}

fn module_receiver(content: &str, mut node: Node<'_>, name: &str, remaining: &mut usize) -> bool {
    let mut access = None;
    while *remaining > 0 {
        *remaining -= 1;
        let Some(receiver) = node
            .child_by_field_name("object")
            .or_else(|| node.child_by_field_name("value"))
        else {
            return false;
        };
        if receiver.kind() == "identifier" {
            if node_text(content, receiver) != name {
                return false;
            }
            if node.kind() != "attribute" {
                return true;
            }
            let member = node
                .child_by_field_name("attribute")
                .map(|n| node_text(content, n));
            return match member.as_deref() {
                Some("overload") => true,
                Some("__dict__") => namespace_may_write_overload(content, access),
                _ => false,
            };
        }
        access = Some(node);
        node = receiver;
    }
    true
}

fn namespace_may_write_overload(content: &str, access: Option<Node<'_>>) -> bool {
    let Some(key) = access
        .filter(|n| n.kind() == "subscript")
        .and_then(|n| n.child_by_field_name("subscript"))
    else {
        return true;
    };
    key_may_name_overload(content, key)
}

fn key_may_name_overload(content: &str, key: Node<'_>) -> bool {
    let literal = node_text(content, key);
    // Unknown/escaped expressions may name overload. Only a plain static key
    // can prove that a different namespace member is being written.
    if key.kind() != "string" || literal.contains('\\') {
        return true;
    }
    let Some(quote) = literal.chars().next().filter(|q| matches!(q, '\'' | '"')) else {
        return true;
    };
    if !literal.ends_with(quote)
        || literal.len() < 2
        || literal.starts_with(&quote.to_string().repeat(3))
    {
        return true;
    }
    &literal[1..literal.len() - 1] == "overload"
}

pub(super) fn expression_mutates_module(
    content: &str,
    expression: Node<'_>,
    binding: &str,
) -> bool {
    let mut cursor = expression.walk();
    for _ in 0..MAX_BINDING_STATEMENTS {
        let node = cursor.node();
        if node.kind() == "call" && mutator_targets_module(content, node, binding) {
            return true;
        }
        if node.kind() != "lambda" && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return false;
            }
        }
    }
    true
}
fn mutator_targets_module(content: &str, node: Node<'_>, binding: &str) -> bool {
    let Some(function) = node
        .child_by_field_name("function")
        .filter(|function| function.kind() == "identifier")
    else {
        return false;
    };
    if !matches!(node_text(content, function).as_str(), "setattr" | "delattr") {
        return false;
    }
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return false;
    };
    let Some(receiver) = arguments
        .named_child(0)
        .filter(|receiver| receiver.kind() == "identifier")
    else {
        return false;
    };
    node_text(content, receiver) == binding
        && arguments
            .named_child(1)
            .is_some_and(|key| key_may_name_overload(content, key))
}

#[cfg(test)]
#[path = "mutations_tests.rs"]
mod tests;
