//! Independent standard-module identities for bounded attribute-read proofs.
use crate::code::parser::nodes::node_text;
use crate::code::python_imports::PythonModuleOrigins;
use tree_sitter::Node;

/// Prove the receiver from its own import, without confusing it with the
/// decorator's separately imported function. Unknown intervening writes and
/// execution boundaries are rejected by the positive preceding-value walk.
pub(super) fn standard(
    content: &str,
    mut receiver: Node<'_>,
    remaining: &mut usize,
    origins: PythonModuleOrigins,
) -> bool {
    loop {
        let Some(value) = super::transparent::transparent(receiver, remaining) else {
            return false;
        };
        if value.kind() != "identifier" {
            return false;
        }
        let name = node_text(content, value);
        let Some(previous) =
            super::proven_aliases::preceding_value(content, value, &name, remaining)
        else {
            return false;
        };
        if previous.kind() != "import_statement" {
            receiver = previous;
            continue;
        }
        let mut cursor = previous.walk();
        for imported in previous.children_by_field_name("name", &mut cursor) {
            let Some(left) = remaining.checked_sub(1) else {
                return false;
            };
            *remaining = left;
            let original = imported.child_by_field_name("name").unwrap_or(imported);
            let local = imported.child_by_field_name("alias").unwrap_or(original);
            if node_text(content, local) == name {
                return origins.permits_standard_module(&node_text(content, original));
            }
        }
        return false;
    }
}

#[cfg(test)]
#[path = "imported_modules_tests.rs"]
mod tests;
