//! Bounded proof of an unshadowed cgo package receiver.
use tree_sitter::Node;

use crate::code::CodeIndexError;

pub(super) fn unshadowed_package(root: Node<'_>, content: &str) -> Result<bool, CodeIndexError> {
    if root.kind() != "source_file" {
        return Ok(false);
    }
    let mut imported = false;
    let mut cursor = root.walk();
    for _ in 0..1_000_000 {
        let node = cursor.node();
        if node.kind() == "import_spec"
            && node.child_by_field_name("name").is_none()
            && node
                .child_by_field_name("path")
                .is_some_and(|path| matches!(&content[path.byte_range()], "\"C\"" | "`C`"))
        {
            imported = true;
        }
        if matches!(
            node.kind(),
            "parameter_declaration"
                | "variadic_parameter_declaration"
                | "var_spec"
                | "const_spec"
                | "short_var_declaration"
                | "range_clause"
                | "function_declaration"
                | "type_spec"
                | "type_alias"
                | "import_spec"
                | "receive_statement"
                | "type_switch_statement"
        ) {
            let mut fields = node.walk();
            if fields.goto_first_child() {
                loop {
                    let binding = fields.node();
                    if matches!(fields.field_name(), Some("name" | "left" | "alias")) {
                        let mut names = binding.walk();
                        if content[binding.byte_range()].trim() == "C"
                            || binding
                                .named_children(&mut names)
                                .any(|name| content[name.byte_range()].trim() == "C")
                        {
                            return Ok(false);
                        }
                    }
                    if !fields.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(imported);
            }
        }
    }
    Err(CodeIndexError::InvalidInput(
        "cgo receiver work budget exceeded".into(),
    ))
}
