//! Index lexical children that can change one variable's export state.
use super::*;
use std::collections::BTreeMap;

pub(super) struct Bindings<'a> {
    children: BTreeMap<(String, usize), Vec<Node<'a>>>,
}

impl<'a> Bindings<'a> {
    pub(super) fn build(root: Node<'a>, content: &str) -> Result<Self, DomainError> {
        let mut result = Self {
            children: BTreeMap::new(),
        };
        let mut work = 0usize;
        let mut cursor = root.walk();
        loop {
            work += 1;
            if work > 1_000_000 {
                return Err(DomainError::invalid(
                    "configuration",
                    "shell binding work budget exceeded",
                ));
            }
            let node = cursor.node();
            let mut names = Vec::new();
            if let Some(name) = match node.kind() {
                "variable_assignment" => node.child_by_field_name("name"),
                "for_statement" => node.child_by_field_name("variable"),
                _ => None,
            } {
                names.push(content[name.byte_range()].to_owned());
            }
            if export_mode(node, content)?.is_some() {
                let mut child_cursor = node.walk();
                for (index, child) in node.named_children(&mut child_cursor).enumerate() {
                    if index >= 1024 {
                        return Err(DomainError::invalid(
                            "configuration",
                            "shell binding operand budget exceeded",
                        ));
                    }
                    if let Some((name, _)) = values::assignment(child, content)? {
                        names.push(name);
                    } else if let Some(name) = values::static_value(Some(child), content)? {
                        names.push(name);
                    }
                }
            }
            for name in names {
                let mut child = node;
                for depth in 0..=128 {
                    work += 1;
                    if work > 1_000_000 {
                        return Err(DomainError::invalid(
                            "configuration",
                            "shell binding work budget exceeded",
                        ));
                    }
                    let Some(parent) = child.parent() else {
                        break;
                    };
                    if parent.kind() == "function_definition" {
                        break;
                    }
                    if depth == 128 {
                        return Err(DomainError::invalid(
                            "configuration",
                            "shell binding lexical depth exceeded",
                        ));
                    }
                    result
                        .children
                        .entry((name.clone(), parent.id()))
                        .or_default()
                        .push(child);
                    child = parent;
                }
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    for children in result.children.values_mut() {
                        children.sort_unstable_by_key(Node::start_byte);
                        children.dedup_by_key(|node| node.id());
                    }
                    return Ok(result);
                }
            }
        }
    }

    pub(super) fn children(&self, key: &str, parent: Node<'_>) -> &[Node<'a>] {
        self.children
            .get(&(key.to_owned(), parent.id()))
            .map_or(&[], Vec::as_slice)
    }
}
