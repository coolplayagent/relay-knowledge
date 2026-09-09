//! Bounded same-file inherited receiver fields and overridden getter contracts.
use super::type_resolution::{is_type, supertype_names, text, visible_parent};
use std::collections::{BTreeSet, VecDeque};
use tree_sitter::Node;

fn parents<'a>(owner: Node<'a>, content: &str, budget: &mut usize) -> Option<Vec<Node<'a>>> {
    let mut out = Vec::new();
    let mut cursor = owner.walk();
    for child in owner.named_children(&mut cursor) {
        *budget = budget.checked_sub(1)?;
        if matches!(
            child.kind(),
            "superclass" | "super_interfaces" | "extends_interfaces"
        ) {
            for name in supertype_names(child, content, budget)? {
                if let Some(parent) = visible_parent(owner, &name, content, budget) {
                    out.push(parent);
                }
                if *budget == 0 {
                    return None;
                }
            }
        }
    }
    Some(out)
}

pub(super) fn receiver_shadowed(mut node: Node<'_>, name: &str, content: &str) -> bool {
    let mut budget = 1024usize;
    let mut queue = VecDeque::new();
    while let Some(parent) = node.parent() {
        let Some(left) = budget.checked_sub(1) else {
            return true;
        };
        budget = left;
        if is_type(parent) {
            let Some(parents) = parents(parent, content, &mut budget) else {
                return true;
            };
            queue.extend(parents);
        }
        node = parent;
    }
    let mut visited = BTreeSet::new();
    while let Some(owner) = queue.pop_front() {
        let Some(left) = budget.checked_sub(1) else {
            return true;
        };
        budget = left;
        if !visited.insert(owner.id()) {
            continue;
        }
        let Some(body) = owner.child_by_field_name("body") else {
            continue;
        };
        let mut cursor = body.walk();
        for field in body.named_children(&mut cursor) {
            let Some(left) = budget.checked_sub(1) else {
                return true;
            };
            budget = left;
            if !matches!(field.kind(), "field_declaration" | "constant_declaration") {
                continue;
            }
            if modifier(field, "private", &mut budget).unwrap_or(false) {
                continue;
            }
            let mut cursor = field.walk();
            for value in field.named_children(&mut cursor) {
                let Some(left) = budget.checked_sub(1) else {
                    return true;
                };
                budget = left;
                if value.kind() == "variable_declarator"
                    && value
                        .child_by_field_name("name")
                        .is_some_and(|n| text(n, content) == name)
                {
                    return true;
                }
            }
        }
        let Some(parents) = parents(owner, content, &mut budget) else {
            return true;
        };
        queue.extend(parents);
    }
    false
}

pub(super) fn getter_contracts<'a>(
    owner: Node<'a>,
    method: Node<'a>,
    name: &str,
    content: &str,
) -> Vec<Node<'a>> {
    let mut budget = 1024usize;
    if modifier(method, "private", &mut budget) != Some(false)
        || modifier(method, "static", &mut budget) != Some(false)
    {
        return Vec::new();
    }
    let Some(initial) = parents(owner, content, &mut budget) else {
        return Vec::new();
    };
    let mut queue = VecDeque::from(initial);
    let mut visited = BTreeSet::new();
    let mut contracts = Vec::new();
    while let Some(parent) = queue.pop_front() {
        let Some(left) = budget.checked_sub(1) else {
            return Vec::new();
        };
        budget = left;
        if !visited.insert(parent.id()) {
            continue;
        }
        let Some(body) = parent.child_by_field_name("body") else {
            continue;
        };
        let mut cursor = body.walk();
        for candidate in body.named_children(&mut cursor) {
            let Some(left) = budget.checked_sub(1) else {
                return Vec::new();
            };
            budget = left;
            if candidate.kind() != "method_declaration"
                || !candidate
                    .child_by_field_name("name")
                    .is_some_and(|n| text(n, content) == name)
                || !candidate
                    .child_by_field_name("parameters")
                    .is_some_and(|n| n.named_child_count() == 0)
            {
                continue;
            }
            if modifier(candidate, "private", &mut budget) == Some(false)
                && modifier(candidate, "static", &mut budget) == Some(false)
            {
                contracts.push(parent);
            }
        }
        let Some(parents) = parents(parent, content, &mut budget) else {
            return Vec::new();
        };
        queue.extend(parents);
    }
    contracts
}

fn modifier(node: Node<'_>, kind: &str, budget: &mut usize) -> Option<bool> {
    if let Some(modifiers) = node.named_child(0).filter(|n| n.kind() == "modifiers") {
        let mut cursor = modifiers.walk();
        for child in modifiers.children(&mut cursor) {
            *budget = budget.checked_sub(1)?;
            if child.kind() == kind {
                return Some(true);
            }
        }
    }
    Some(false)
}

#[cfg(test)]
#[path = "inherited_members_tests.rs"]
mod tests;
