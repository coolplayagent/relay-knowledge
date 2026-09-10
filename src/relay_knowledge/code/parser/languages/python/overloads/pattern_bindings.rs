//! Capture targets in match patterns, excluding value and class references.
use crate::code::parser::nodes::node_text;
use tree_sitter::Node;

/// Unknown or exhausted pattern analysis may bind the queried name.
pub(super) fn binds(content: &str, pattern: Node<'_>, name: &str, remaining: &mut usize) -> bool {
    let mut stack = vec![pattern];
    while let Some(node) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return true;
        };
        *remaining = left;
        match node.kind() {
            "identifier" => {
                if node_text(content, node) == name && name != "_" {
                    return true;
                }
                continue;
            }
            "dotted_name" if node.named_child_count() != 1 => continue,
            "case_clause" | "case_pattern" | "dotted_name" | "list_pattern" | "tuple_pattern"
            | "union_pattern" | "as_pattern" | "splat_pattern" | "dict_pattern"
            | "class_pattern" | "keyword_pattern" => {}
            _ => continue,
        }
        let mut cursor = node.walk();
        for (index, child) in node.named_children(&mut cursor).enumerate() {
            let Some(left) = remaining.checked_sub(1) else {
                return true;
            };
            *remaining = left;
            let capture = match node.kind() {
                "case_clause" => child.kind() == "case_pattern",
                "dict_pattern" => matches!(child.kind(), "case_pattern" | "splat_pattern"),
                "class_pattern" | "keyword_pattern" => index != 0,
                _ => true,
            };
            if capture {
                stack.push(child);
            }
        }
    }
    false
}

#[cfg(test)]
#[path = "pattern_bindings_tests.rs"]
mod tests;
