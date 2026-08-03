//! C declaration, type, data, and direct-function symbol materialization.

//! C declaration symbol classification and materialization.

use tree_sitter::Node;

use super::lexical::data_symbol_name;
use crate::code::parser::nodes::{SyntaxRange, node_text, push_children_reverse, syntax_range};

const MAX_TOP_LEVEL_DATA_SYMBOL_LINES: usize = 80;

pub(super) fn direct_function_definition_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    node.child_by_field_name("declarator")
        .and_then(|declarator| declarator_name(content, declarator))
        .map(|name| (name, "function", syntax_range(node)))
}

pub(super) fn direct_composite_type_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !matches!(
        node.kind(),
        "enum_specifier" | "struct_specifier" | "union_specifier"
    ) || !composite_type_is_top_level(node)
        || !node_text(content, node).contains('{')
    {
        return None;
    }
    let name = node.child_by_field_name("name")?;
    let name = node_text(content, name);

    data_symbol_name(&name).then(|| (name, "type", syntax_range(node)))
}

fn composite_type_is_top_level(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "translation_unit" => return true,
            "declaration" => {}
            kind if kind.starts_with("preproc_") => {}
            _ => return false,
        }
        node = parent;
    }

    false
}

pub(super) fn top_level_declaration_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    if declaration.kind() == "type_definition" || is_typedef_declaration(content, declaration) {
        return typedef_type_symbols(content, declaration);
    }

    let mut symbols = enum_type_symbols(content, declaration);
    symbols.extend(function_declaration_symbols(content, declaration));
    symbols.extend(top_level_data_symbols(content, declaration));
    symbols
}

fn enum_type_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let mut cursor = declaration.walk();
    declaration
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "enum_specifier")
        .filter(|child| node_text(content, *child).contains('{'))
        .filter_map(|child| {
            let name = child.child_by_field_name("name")?;
            let name = node_text(content, name);
            data_symbol_name(&name).then(|| (name, "type", syntax_range(child)))
        })
        .collect()
}

fn typedef_type_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let range = syntax_range(declaration);
    let mut cursor = declaration.walk();

    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| declarator_name(content, declarator))
        .filter(|name| data_symbol_name(name))
        .map(|name| (name, "type", range.clone()))
        .collect()
}

fn top_level_data_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let range = syntax_range(declaration);
    if range.line_end.saturating_sub(range.line_start) > MAX_TOP_LEVEL_DATA_SYMBOL_LINES {
        return Vec::new();
    }
    let composite_type = declaration_has_composite_type(content, declaration);
    let initializer_contract_type = declaration_has_initializer_contract_type(content, declaration);
    let mut cursor = declaration.walk();

    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| {
            initialized_data_declarator_name(
                content,
                declarator,
                composite_type,
                initializer_contract_type,
            )
        })
        .map(|name| (name, "constant", range.clone()))
        .collect()
}

fn initialized_data_declarator_name(
    content: &str,
    declarator: Node<'_>,
    composite_type: bool,
    initializer_contract_type: bool,
) -> Option<String> {
    if declarator.kind() != "init_declarator" {
        return None;
    }
    let value = declarator.child_by_field_name("value")?;
    if !matches!(value.kind(), "initializer_list" | "call_expression") {
        return None;
    }
    let inner = declarator.child_by_field_name("declarator")?;
    let array_declarator = contains_node_kind(inner, "array_declarator");
    let typedef_initializer = initializer_contract_type && value.kind() == "initializer_list";
    if !composite_type && !array_declarator && !typedef_initializer {
        return None;
    }
    if contains_node_kind(inner, "function_declarator") {
        return None;
    }

    declarator_name(content, inner).filter(|name| data_symbol_name(name))
}

fn declaration_has_composite_type(content: &str, declaration: Node<'_>) -> bool {
    declaration
        .child_by_field_name("type")
        .is_some_and(|type_node| {
            matches!(
                type_node.kind(),
                "struct_specifier" | "union_specifier" | "enum_specifier"
            ) || {
                let type_text = node_text(content, type_node);
                type_text.starts_with("struct ")
                    || type_text.starts_with("union ")
                    || type_text.starts_with("enum ")
            }
        })
}

fn declaration_has_initializer_contract_type(content: &str, declaration: Node<'_>) -> bool {
    declaration
        .child_by_field_name("type")
        .is_some_and(|type_node| typedef_like_contract_type(&node_text(content, type_node)))
}

fn typedef_like_contract_type(name: &str) -> bool {
    name.split_whitespace()
        .last()
        .is_some_and(c_external_contract_type_token)
}

fn c_external_contract_type_token(token: &str) -> bool {
    (token.ends_with("_t") && data_symbol_name(token))
        || (token
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
            && token
                .chars()
                .any(|character| character.is_ascii_lowercase())
            && data_symbol_name(token))
}

pub(super) fn decorated_cpp_class_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let text = node_text(content, node);
    let head = text.split('{').next()?.trim();
    let tail = head.strip_prefix("class ")?;
    let declaration = tail.split(':').next().unwrap_or(tail);
    let name = declaration
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .rfind(|token| cpp_class_name_candidate(token))?;

    Some((name.to_owned(), "class", syntax_range(node)))
}

fn cpp_class_name_candidate(token: &str) -> bool {
    if token.is_empty() || matches!(token, "final") {
        return false;
    }
    let mut characters = token.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn function_declaration_symbols(
    content: &str,
    declaration: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let mut cursor = declaration.walk();
    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| {
            let function_declarator = direct_function_declarator(declarator)?;
            let name = declarator_name(content, function_declarator)?;

            Some((name, "function_declaration", syntax_range(declaration)))
        })
        .collect()
}

fn is_typedef_declaration(content: &str, declaration: Node<'_>) -> bool {
    let mut stack = vec![declaration];
    while let Some(node) = stack.pop() {
        if node.kind() == "storage_class_specifier" && node_text(content, node) == "typedef" {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }

    false
}

fn direct_function_declarator(declarator: Node<'_>) -> Option<Node<'_>> {
    let mut stack = vec![declarator];
    while let Some(node) = stack.pop() {
        if node.kind() == "parameter_declaration" {
            continue;
        }
        if node.kind() == "function_declarator" && !is_function_pointer_variable(node) {
            return Some(node);
        }
        push_children_reverse(node, &mut stack);
    }

    None
}

fn is_function_pointer_variable(function_declarator: Node<'_>) -> bool {
    function_declarator
        .child_by_field_name("declarator")
        .is_some_and(has_parenthesized_pointer_declarator)
}

fn declarator_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(
            current.kind(),
            "identifier" | "field_identifier" | "type_identifier"
        ) {
            return Some(node_text(content, current));
        }
        if let Some(declarator) = current.child_by_field_name("declarator") {
            stack.push(declarator);
            continue;
        }
        push_children_reverse(current, &mut stack);
    }

    None
}

fn contains_node_kind(root: Node<'_>, kind: &str) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }

    false
}

fn has_parenthesized_pointer_declarator(root: Node<'_>) -> bool {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "parenthesized_declarator"
            && contains_node_kind(node, "pointer_declarator")
        {
            return true;
        }
        push_children_reverse(node, &mut stack);
    }

    false
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
