use tree_sitter::Node;

use super::super::nodes::{SyntaxRange, first_named_child_of_kind, node_text, syntax_range};
use super::super::recovery::{
    decorated_function_error_body_is_statement_like, decorated_function_head_has_recoverable_tail,
    decorated_function_head_has_recovery_decorator, decorated_function_head_text,
};

mod cpp_header_recovery;
mod declaration_symbols;
mod gcc_recovery;
mod lexical;
mod macro_functions;
mod node_kinds;
mod preprocessor;

pub(in crate::code::parser) use cpp_header_recovery::manual_file_definitions;
use declaration_symbols::{
    decorated_cpp_class_symbol, direct_composite_type_symbol, direct_function_definition_symbol,
    top_level_declaration_symbols,
};
use gcc_recovery::gcc_decorated_function_symbol;
use macro_functions::{
    MacroBodyFunctionDefinition, macro_body_function_definition,
    macro_generated_function_definition, syscall_macro_definition,
};
pub(in crate::code::parser) use node_kinds::{definition_kind, is_call_node};

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "function_definition" => match macro_body_function_definition(content, node) {
            MacroBodyFunctionDefinition::Recovered(definition) => vec![definition],
            MacroBodyFunctionDefinition::Rejected => Vec::new(),
            MacroBodyFunctionDefinition::NotMacroBody => {
                if let Some(symbol) = gcc_decorated_function_symbol(content, node) {
                    return vec![symbol];
                }
                if let Some(symbol) = decorated_cpp_class_symbol(content, node) {
                    return vec![symbol];
                }
                if function_definition_has_unrecoverable_decorator_shape(content, node) {
                    return Vec::new();
                }
                if node.has_error() {
                    return Vec::new();
                }
                direct_function_definition_symbol(content, node)
                    .map(|symbol| vec![symbol])
                    .unwrap_or_default()
            }
        },
        "ERROR" if !has_ancestor_kind(node, "compound_statement") => {
            gcc_decorated_function_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "type_definition" | "declaration" if !has_ancestor_kind(node, "compound_statement") => {
            top_level_declaration_symbols(content, node)
        }
        "enum_specifier" | "struct_specifier" | "union_specifier" => {
            direct_composite_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "preproc_def" | "preproc_function_def" => node
            .child_by_field_name("name")
            .or_else(|| first_named_child_of_kind(node, "identifier"))
            .map(|name| vec![(node_text(content, name), "macro", syntax_range(node))])
            .unwrap_or_default(),
        "call_expression" if !has_ancestor_kind(node, "compound_statement") => {
            syscall_macro_definition(content, node)
                .or_else(|| macro_generated_function_definition(content, node))
                .map(|definition| vec![definition])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn function_definition_has_unrecoverable_decorator_shape(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    decorated_function_head_text(&text).is_some_and(|head| {
        decorated_function_head_has_recovery_decorator(head)
            && (!decorated_function_head_has_recoverable_tail(head, false, false, false)
                || !decorated_function_error_body_is_statement_like(&text))
    })
}

fn has_ancestor_kind(mut node: Node<'_>, kind: &str) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return true;
        }
        node = parent;
    }

    false
}
