use tree_sitter::Node;

use super::nodes::{SyntaxRange, node_text, push_children_reverse, syntax_range};

pub(super) fn manual_definitions(
    content: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    match node.kind() {
        "class_specifier" | "struct_specifier" => decorated_cpp_type_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        "declaration" => decorated_cpp_type_symbol(content, node)
            .map(|symbol| vec![symbol])
            .unwrap_or_default(),
        "function_definition" => decorated_cpp_type_symbol(content, node)
            .map(|symbol| vec![symbol])
            .or_else(|| {
                node.child_by_field_name("declarator")
                    .and_then(|declarator| declarator_name(content, declarator))
                    .map(|name| vec![(name, "function", syntax_range(node))])
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn decorated_cpp_type_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let text = node_text(content, node);
    let head = text
        .split(['{', ';'])
        .next()
        .unwrap_or(text.as_str())
        .trim();
    let mut tokens = head
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty());
    while let Some(token) = tokens.next() {
        let kind = match token {
            "class" => "class",
            "struct" | "union" | "enum" => "type",
            _ => continue,
        };
        let name = tokens.find(|candidate| cpp_type_name_candidate(candidate))?;
        return Some((name.to_owned(), kind, syntax_range(node)));
    }

    None
}

fn cpp_type_name_candidate(token: &str) -> bool {
    if matches!(
        token,
        "final" | "public" | "private" | "protected" | "virtual"
    ) {
        return false;
    }
    if cpp_decorator_token(token) {
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

fn declarator_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(
            current.kind(),
            "identifier" | "field_identifier" | "operator_name"
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
