use tree_sitter::Node;

use super::nodes::{
    SyntaxRange, first_named_child_of_kind, node_text, push_children_reverse, syntax_range,
};

const MAX_TOP_LEVEL_DATA_SYMBOL_LINES: usize = 80;

pub(super) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "function_definition" => decorated_cpp_class_symbol(content, node)
            .map(|symbol| vec![symbol])
            .or_else(|| {
                node.child_by_field_name("declarator")
                    .and_then(|declarator| declarator_name(content, declarator))
                    .map(|name| vec![(name, "function", syntax_range(node))])
            })
            .unwrap_or_default(),
        "type_definition" if !has_ancestor_kind(node, "compound_statement") => {
            typedef_type_symbols(content, node)
        }
        "declaration" if !has_ancestor_kind(node, "compound_statement") => {
            if is_typedef_declaration(content, node) {
                typedef_type_symbols(content, node)
            } else {
                let mut symbols = function_declaration_symbols(content, node);
                symbols.extend(top_level_data_symbols(content, node));
                symbols
            }
        }
        "preproc_def" | "preproc_function_def" => node
            .child_by_field_name("name")
            .or_else(|| first_named_child_of_kind(node, "identifier"))
            .map(|name| vec![(node_text(content, name), "macro", syntax_range(node))])
            .unwrap_or_default(),
        "call_expression" if !has_ancestor_kind(node, "compound_statement") => {
            syscall_macro_definition(content, node)
                .map(|definition| vec![definition])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
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
    let mut cursor = declaration.walk();

    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|declarator| {
            initialized_data_declarator_name(content, declarator, composite_type)
        })
        .map(|name| (name, "constant", range.clone()))
        .collect()
}

fn initialized_data_declarator_name(
    content: &str,
    declarator: Node<'_>,
    composite_type: bool,
) -> Option<String> {
    if declarator.kind() != "init_declarator" {
        return None;
    }
    let value = declarator.child_by_field_name("value")?;
    if !matches!(value.kind(), "initializer_list" | "call_expression") {
        return None;
    }
    let inner = declarator.child_by_field_name("declarator")?;
    if !composite_type && !contains_node_kind(inner, "array_declarator") {
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

fn data_symbol_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn decorated_cpp_class_symbol(
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
    if token.is_empty() || matches!(token, "final") || cpp_decorator_token(token) {
        return false;
    }
    let mut characters = token.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn cpp_decorator_token(token: &str) -> bool {
    token.starts_with("__")
        || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
        || (token.chars().any(|character| character == '_')
            && token
                .chars()
                .all(|character| character == '_' || character.is_ascii_uppercase()))
}

fn syscall_macro_definition(
    content: &str,
    call: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let function = call.child_by_field_name("function")?;
    let macro_name = node_text(content, function);
    if !is_syscall_definition_macro(&macro_name) {
        return None;
    }
    let arguments = call.child_by_field_name("arguments")?;
    let syscall_name = first_named_child_of_kind(arguments, "identifier")?;

    Some((
        node_text(content, syscall_name),
        "function",
        syntax_range(call),
    ))
}

fn is_syscall_definition_macro(name: &str) -> bool {
    let Some(suffix) = name
        .strip_prefix("SYSCALL_DEFINE")
        .or_else(|| name.strip_prefix("COMPAT_SYSCALL_DEFINE"))
    else {
        return false;
    };

    !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
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

fn has_ancestor_kind(mut node: Node<'_>, kind: &str) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return true;
        }
        node = parent;
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
