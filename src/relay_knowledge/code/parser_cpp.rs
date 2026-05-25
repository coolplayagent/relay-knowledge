use tree_sitter::Node;

use super::nodes::{SyntaxRange, node_text, push_children_reverse, syntax_range};

pub(super) fn manual_definitions(
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
        "function_definition"
            if decorated_declaration_head_starts_with_type_definition(content, node) =>
        {
            decorated_cpp_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "function_definition" => node
            .child_by_field_name("declarator")
            .and_then(|declarator| declarator_name(content, declarator))
            .map(|name| vec![(name, "function", syntax_range(node))])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn decorated_cpp_declaration_type_symbol(
    content: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    let type_node = direct_definition_type_specifier(content, node)?;
    decorated_cpp_type_symbol(content, type_node)
        .or_else(|| decorated_cpp_type_symbol(content, node))
        .map(|(name, kind, _)| (name, kind, syntax_range(node)))
}

fn direct_definition_type_specifier<'tree>(
    content: &str,
    node: Node<'tree>,
) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| {
            matches!(
                child.kind(),
                "class_specifier" | "enum_specifier" | "struct_specifier" | "union_specifier"
            ) && cpp_type_declaration_context(content, *child)
        })
        .or_else(|| {
            decorated_declaration_head_starts_with_type_definition(content, node).then_some(node)
        })
}

fn cpp_type_declaration_context(content: &str, node: Node<'_>) -> bool {
    if node_text(content, node).contains('{') {
        return true;
    }
    let Some(parent) = node
        .parent()
        .filter(|parent| parent.kind() == "declaration")
    else {
        return false;
    };
    content
        .get(node.end_byte()..parent.end_byte())
        .is_some_and(|trailing| trailing.trim() == ";")
}

fn decorated_declaration_head_starts_with_type_definition(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    if !text.contains('{') {
        return false;
    }
    let head = text
        .split(['{', ';'])
        .next()
        .unwrap_or(text.as_str())
        .trim();
    let tokens = head
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty());
    for token in tokens {
        if cpp_type_intro_keyword(token) {
            return true;
        }
        if cpp_declaration_prefix_token(token) {
            continue;
        }
        return false;
    }

    false
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
    if cpp_keyword_token(token) {
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

fn cpp_type_intro_keyword(token: &str) -> bool {
    matches!(token, "class" | "struct" | "union" | "enum")
}

fn cpp_declaration_prefix_token(token: &str) -> bool {
    cpp_decorator_token(token)
        || matches!(
            token,
            "alignas"
                | "constexpr"
                | "export"
                | "extern"
                | "friend"
                | "inline"
                | "static"
                | "template"
                | "typename"
                | "using"
        )
}

fn cpp_keyword_token(token: &str) -> bool {
    matches!(
        token,
        "alignas"
            | "class"
            | "const"
            | "constexpr"
            | "enum"
            | "explicit"
            | "export"
            | "extern"
            | "final"
            | "friend"
            | "inline"
            | "mutable"
            | "namespace"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "struct"
            | "template"
            | "typename"
            | "union"
            | "using"
            | "virtual"
            | "volatile"
    )
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
