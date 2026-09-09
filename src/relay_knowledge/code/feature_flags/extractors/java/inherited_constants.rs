//! Bounded same-tree inheritance proof for an unqualified constant reference.
use super::{
    type_resolution::{import_path, supertype_names, visible_parent},
    type_resolution::{is_type, text},
};
use std::collections::{BTreeSet, VecDeque};
use tree_sitter::Node;

const MAX_RESOLUTION_NODES: usize = 1024;

pub(super) fn symbol(owner: Node<'_>, name: &str, content: &str) -> Option<String> {
    let mut remaining = MAX_RESOLUTION_NODES;
    let mut queue = VecDeque::new();
    parents(owner, content, &mut remaining, &mut queue)?;
    let mut visited = BTreeSet::new();
    let mut matches = BTreeSet::new();
    while let Some(parent) = queue.pop_front() {
        remaining = remaining.checked_sub(1)?;
        if !visited.insert(parent.id()) {
            continue;
        }
        match declaration(parent, name, content, &mut remaining)? {
            Some(true) => {
                matches.insert(identity(parent, name, content, &mut remaining)?);
            }
            Some(false) => return None,
            None => parents(parent, content, &mut remaining, &mut queue)?,
        }
        if matches.len() > 1 {
            return None;
        }
    }
    (matches.len() == 1)
        .then(|| matches.into_iter().next())
        .flatten()
}

fn parents<'a>(
    owner: Node<'a>,
    content: &str,
    remaining: &mut usize,
    queue: &mut VecDeque<Node<'a>>,
) -> Option<()> {
    let mut cursor = owner.walk();
    for child in owner.named_children(&mut cursor) {
        *remaining = remaining.checked_sub(1)?;
        if !matches!(
            child.kind(),
            "superclass" | "super_interfaces" | "extends_interfaces"
        ) {
            continue;
        }
        for name in supertype_names(child, content, remaining)? {
            if name == "java.lang.Object" {
                continue;
            }
            queue.push_back(visible_parent(owner, &name, content, remaining)?);
        }
    }
    Some(())
}

// A declared name hides deeper ancestors even when it is not an accessible
// static constant. Do not invent a binding through that conflicting declaration.
fn declaration(
    owner: Node<'_>,
    name: &str,
    content: &str,
    remaining: &mut usize,
) -> Option<Option<bool>> {
    let mut queue = vec![owner.child_by_field_name("body")?];
    while let Some(body) = queue.pop() {
        *remaining = remaining.checked_sub(1)?;
        let mut cursor = body.walk();
        for field in body.named_children(&mut cursor) {
            *remaining = remaining.checked_sub(1)?;
            if field.kind() == "enum_body_declarations" {
                queue.push(field);
                continue;
            }
            if !matches!(field.kind(), "field_declaration" | "constant_declaration") {
                continue;
            }
            let mut declarators = field.walk();
            for value in field.named_children(&mut declarators) {
                *remaining = remaining.checked_sub(1)?;
                if value.kind() == "variable_declarator"
                    && value
                        .child_by_field_name("name")
                        .is_some_and(|n| text(n, content) == name)
                {
                    return Some(Some(accessible_constant(field, remaining)?));
                }
            }
        }
    }
    Some(None)
}

fn accessible_constant(field: Node<'_>, remaining: &mut usize) -> Option<bool> {
    let mut is_static = field.kind() == "constant_declaration";
    let mut is_final = is_static;
    if let Some(modifiers) = field.named_child(0).filter(|n| n.kind() == "modifiers") {
        let mut cursor = modifiers.walk();
        for modifier in modifiers.children(&mut cursor) {
            *remaining = remaining.checked_sub(1)?;
            if modifier.kind() == "private" {
                return Some(false);
            }
            is_static |= modifier.kind() == "static";
            is_final |= modifier.kind() == "final";
        }
    }
    Some(is_static && is_final)
}

fn identity(
    mut owner: Node<'_>,
    name: &str,
    content: &str,
    remaining: &mut usize,
) -> Option<String> {
    let mut names = vec![name.to_owned()];
    loop {
        *remaining = remaining.checked_sub(1)?;
        if is_type(owner) {
            names.push(text(owner.child_by_field_name("name")?, content).to_owned());
        }
        match owner.parent() {
            Some(parent) => owner = parent,
            None => break,
        }
    }
    names.reverse();
    let mut cursor = owner.walk();
    for child in owner.named_children(&mut cursor) {
        *remaining = remaining.checked_sub(1)?;
        if child.kind() == "package_declaration" {
            names.insert(0, import_path(child, content, remaining)?.0);
            break;
        }
    }
    Some(names.join("."))
}

#[cfg(test)]
#[path = "inherited_constants_tests.rs"]
mod tests;
