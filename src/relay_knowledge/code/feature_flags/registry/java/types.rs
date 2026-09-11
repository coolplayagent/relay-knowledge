//! Lexical nested type qualification and bounded same-file supertype closure.
use super::names;
use crate::domain::DomainError;
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;
fn is_type(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration"
    )
}
pub(super) fn lexical(mut node: Node<'_>, head: &str, content: &str) -> Option<String> {
    let position = node.start_byte();
    let mut budget = 4096usize;
    while let Some(parent) = node.parent() {
        budget = budget.checked_sub(1)?;
        if is_type(parent)
            && parent
                .child_by_field_name("name")
                .is_some_and(|n| names::text(n, content) == head)
        {
            return Some(names::field_symbol(parent, head, content));
        }
        if matches!(parent.kind(), "class_body" | "interface_body" | "block") {
            let mut cursor = parent.walk();
            for declaration in parent.named_children(&mut cursor) {
                budget = budget.checked_sub(1)?;
                if is_type(declaration)
                    && (parent.kind() != "block" || declaration.start_byte() < position)
                    && declaration
                        .child_by_field_name("name")
                        .is_some_and(|n| names::text(n, content) == head)
                {
                    return Some(names::field_symbol(declaration, head, content));
                }
            }
        }
        node = parent;
    }
    None
}
pub(super) struct Hierarchy(BTreeMap<String, Vec<String>>);
impl Hierarchy {
    pub(super) fn collect(root: Node<'_>, content: &str) -> Result<Self, DomainError> {
        let mut graph = BTreeMap::new();
        let mut cursor = root.walk();
        let mut budget = 100_000usize;
        loop {
            budget = budget.checked_sub(1).ok_or_else(|| {
                DomainError::invalid(
                    "configuration",
                    "Java type analysis incomplete: node budget exceeded",
                )
            })?;
            let node = cursor.node();
            if is_type(node) {
                let mut parents = Vec::new();
                let mut child_cursor = node.walk();
                for clause in node.named_children(&mut child_cursor).filter(|n| {
                    matches!(
                        n.kind(),
                        "super_interfaces" | "superclass" | "extends_interfaces"
                    )
                }) {
                    let mut pending = vec![clause];
                    while let Some(ty) = pending.pop() {
                        budget = budget.checked_sub(1).ok_or_else(|| {
                            DomainError::invalid(
                                "configuration",
                                "Java type analysis incomplete: node budget exceeded",
                            )
                        })?;
                        match ty.kind() {
                            "generic_type" => {
                                if let Some(base) = ty.named_child(0) {
                                    pending.push(base);
                                }
                            }
                            "type_identifier" | "scoped_type_identifier" => parents
                                .push(names::qualified(node, names::text(ty, content), content)),
                            _ => {
                                let mut walk = ty.walk();
                                pending.extend(ty.named_children(&mut walk));
                            }
                        }
                        if parents.len() > 64 || pending.len() > 1024 {
                            return Err(DomainError::invalid(
                                "configuration",
                                "Java supertype analysis incomplete: breadth budget exceeded",
                            ));
                        }
                    }
                }
                if let Some(name) = node.child_by_field_name("name") {
                    graph.insert(
                        names::field_symbol(node, names::text(name, content), content),
                        parents,
                    );
                }
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return Ok(Self(graph));
                }
            }
        }
    }
    pub(super) fn fact(
        &self,
        input: &super::FeatureFlagFileInput<'_>,
        node: Node<'_>,
    ) -> Result<Option<crate::domain::CodeFeatureFlagRecord>, DomainError> {
        if !is_type(node) {
            return Ok(None);
        }
        let Some(name) = node.child_by_field_name("name") else {
            return Ok(None);
        };
        let owner = names::field_symbol(node, names::text(name, input.content), input.content);
        let Some(parents) = self.0.get(&owner).filter(|parents| !parents.is_empty()) else {
            return Ok(None);
        };
        let end = node
            .child_by_field_name("body")
            .map_or(name.end_byte(), |body| body.start_byte());
        let mut row = super::super::record(
            input,
            "config_symbol",
            &owner,
            "config_type_hierarchy",
            node.start_byte(),
            end,
        )?;
        row.metadata.bindings = parents.clone();
        Ok(Some(row))
    }
    pub(super) fn bindings(
        &self,
        method: Node<'_>,
        content: &str,
    ) -> Result<Vec<String>, DomainError> {
        let Some(name) = method.child_by_field_name("name") else {
            return Ok(Vec::new());
        };
        let name = names::text(name, content);
        let owner = names::field_symbol(method, "", content)
            .trim_end_matches('.')
            .to_owned();
        let mut result = vec![format!("{owner}.{name}")];
        let mut seen = BTreeSet::from([owner.clone()]);
        let mut pending = vec![owner];
        while let Some(owner) = pending.pop() {
            for parent in self.0.get(&owner).into_iter().flatten() {
                if seen.insert(parent.clone()) {
                    if seen.len() > 64 {
                        return Err(DomainError::invalid(
                            "configuration",
                            "Java supertype analysis incomplete: closure budget exceeded",
                        ));
                    }
                    result.push(format!("{parent}.{name}"));
                    pending.push(parent.clone());
                }
            }
        }
        Ok(result)
    }
}
