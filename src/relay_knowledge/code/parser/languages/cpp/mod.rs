mod manual;
mod node_kinds;

pub(in crate::code::parser) use manual::{function_definition_is_destructor, manual_definitions};
pub(in crate::code::parser) use node_kinds::{definition_kind, is_call_node};

// C++ tags capture declarators without their terminator. Classify prototypes
// from their AST owner while preserving existing symbol ranges and doc anchors.
pub(in crate::code::parser) fn is_callable_declaration(mut node: tree_sitter::Node<'_>) -> bool {
    if node.kind() != "function_declarator" {
        return false;
    }
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "declaration" | "field_declaration" => return true,
            "function_definition"
            | "parameter_declaration"
            | "class_specifier"
            | "namespace_definition"
            | "translation_unit" => return false,
            _ => node = parent,
        }
    }
    false
}
