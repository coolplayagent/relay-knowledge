//! Resolve implicit getter receivers only from visible, declared zero-argument methods.
use super::names;
use tree_sitter::Node;
pub(super) fn owner(mut node: Node<'_>, method: &str, content: &str) -> Option<String> {
    let mut budget = 4096usize;
    while let Some(parent) = node.parent() {
        budget = budget.checked_sub(1)?;
        if matches!(parent.kind(), "class_body" | "interface_body" | "enum_body") {
            // Anonymous owners cannot be represented by the enclosing named class.
            if parent
                .parent()
                .is_some_and(|p| p.kind() == "object_creation_expression")
            {
                return None;
            }
            let mut cursor = parent.walk();
            let mut candidates = 0;
            let mut matching_name = false;
            for member in parent.named_children(&mut cursor) {
                budget = budget.checked_sub(1)?;
                if member.kind() != "method_declaration"
                    || !member
                        .child_by_field_name("name")
                        .is_some_and(|n| names::text(n, content) == method)
                {
                    continue;
                }
                matching_name = true;
                if member
                    .child_by_field_name("parameters")
                    .is_some_and(|p| p.named_child_count() == 0)
                {
                    candidates += 1;
                }
            }
            if matching_name {
                return (candidates == 1).then(|| {
                    names::field_symbol(node, "", content)
                        .trim_end_matches('.')
                        .to_owned()
                });
            }
            // An inherited method may shadow an outer owner's method.
            if parent.parent().is_some_and(|p| {
                p.child_by_field_name("superclass").is_some()
                    || p.child_by_field_name("interfaces").is_some()
            }) {
                return None;
            }
        }
        node = parent;
    }
    None
}
