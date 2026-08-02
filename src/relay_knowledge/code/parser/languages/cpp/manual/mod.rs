use tree_sitter::Node;

use super::super::super::nodes::SyntaxRange;
use function_definitions::{function_definition_symbol, gcc_decorated_function_symbol};
use type_definitions::{
    cpp_type_declaration_context, decorated_cpp_declaration_type_symbol, decorated_cpp_type_symbol,
    decorated_declaration_head_declares_function,
    decorated_declaration_head_starts_with_type_definition,
};

mod function_definitions;
mod lexical;
mod type_definitions;

pub(in crate::code::parser) use function_definitions::function_definition_is_destructor;

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "class_specifier" | "enum_specifier" | "struct_specifier" | "union_specifier"
            if cpp_type_declaration_context(content, node) =>
        {
            decorated_cpp_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "declaration" => decorated_cpp_declaration_type_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        "ERROR" if decorated_declaration_head_starts_with_type_definition(content, node) => {
            decorated_cpp_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "ERROR" => gcc_decorated_function_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        "function_definition"
            if decorated_declaration_head_starts_with_type_definition(content, node)
                && (node.child_by_field_name("declarator").is_none()
                    || !decorated_declaration_head_declares_function(content, node)) =>
        {
            decorated_cpp_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "function_definition" if function_definition_is_destructor(content, node) => Vec::new(),
        "function_definition" if node.has_error() => gcc_decorated_function_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        "function_definition" => function_definition_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
