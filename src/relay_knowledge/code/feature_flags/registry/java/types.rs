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
    pub(super) fn facts(
        &self,
        input: &super::FeatureFlagFileInput<'_>,
        node: Node<'_>,
    ) -> Result<Vec<crate::domain::CodeFeatureFlagRecord>, DomainError> {
        if !is_type(node) {
            return Ok(Vec::new());
        }
        let Some(name) = node.child_by_field_name("name") else {
            return Ok(Vec::new());
        };
        let owner = names::field_symbol(node, names::text(name, input.content), input.content);
        let Some(parents) = self.0.get(&owner) else {
            return Ok(Vec::new());
        };
        let end = node
            .child_by_field_name("body")
            .map_or(name.end_byte(), |body| body.start_byte());
        let mut facts = Vec::new();
        if !parents.is_empty() {
            let mut row = super::super::record(
                input,
                "config_symbol",
                &owner,
                "config_type_hierarchy",
                node.start_byte(),
                end,
            )?;
            row.metadata.bindings = parents.clone();
            facts.push(row);
        }
        if matches!(
            names::text(name, input.content),
            "System" | "Boolean" | "Integer" | "Long" | "Double"
        ) {
            facts.push(super::super::record(
                input,
                "config_symbol",
                &owner,
                "config_type_declaration",
                node.start_byte(),
                end,
            )?);
        }
        Ok(facts)
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
        if !overridable(method, content) {
            return Ok(result);
        }
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

pub(super) fn overridable(method: Node<'_>, content: &str) -> bool {
    let mut cursor = method.walk();
    !method.named_children(&mut cursor).any(|child| {
        child.kind() == "modifiers"
            && names::text(child, content)
                .split_whitespace()
                .any(|word| matches!(word, "static" | "private"))
    })
}
pub(super) fn static_receiver(node: Node<'_>, content: &str) -> Option<String> {
    let raw = names::text(node, content);
    let head = raw.split('.').next()?;
    if names::binding(node, head, content).is_some() {
        return None;
    }
    let mut root = node;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    let mut visible = lexical(node, head, content).is_some();
    let mut cursor = root.walk();
    for item in root.named_children(&mut cursor).take(4096) {
        if is_type(item)
            && item
                .child_by_field_name("name")
                .is_some_and(|n| names::text(n, content) == head)
        {
            visible = true;
        }
        if item.kind() == "import_declaration" {
            let imported = names::text(item, content)
                .trim()
                .trim_start_matches("import ")
                .trim_end_matches(';')
                .trim();
            if let Some(path) = imported.strip_prefix("static ") {
                if path.ends_with(".*") || path.rsplit('.').next() == Some(head) {
                    return None;
                }
            } else if imported.rsplit('.').next() == Some(head) {
                visible = true;
            }
        }
    }
    // A fully qualified type expression has no lexical variable at its head.
    if !visible && !(raw.contains('.') && head.chars().next().is_some_and(char::is_lowercase)) {
        return None;
    }
    Some(names::qualified(node, raw, content))
}
pub(super) fn platform_shadow(mut node: Node<'_>, owner: &str, content: &str) -> Option<String> {
    if !matches!(owner, "System" | "Boolean" | "Integer" | "Long" | "Double") {
        return None;
    }
    while let Some(parent) = node.parent() {
        node = parent;
    }
    let mut package = String::new();
    let mut cursor = node.walk();
    for item in node.named_children(&mut cursor).take(4096) {
        let text = names::text(item, content)
            .trim()
            .trim_end_matches(';')
            .trim();
        if item.kind() == "import_declaration"
            && text
                .strip_prefix("import ")
                .is_some_and(|p| p.trim() == format!("java.lang.{owner}"))
        {
            return None;
        }
        if item.kind() == "package_declaration" {
            package = text.trim_start_matches("package").trim().to_owned();
        }
    }
    Some(if package.is_empty() {
        owner.into()
    } else {
        format!("{package}.{owner}")
    })
}
