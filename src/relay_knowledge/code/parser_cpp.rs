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
        "ERROR" if decorated_declaration_head_starts_with_type_definition(content, node) => {
            decorated_cpp_type_symbol(content, node)
                .map(|symbol| vec![symbol])
                .unwrap_or_default()
        }
        "function_definition"
            if (node.child_by_field_name("declarator").is_none()
                || decorated_declaration_head_has_type_prefix(content, node))
                && decorated_declaration_head_starts_with_type_definition(content, node) =>
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
    for token in cpp_head_tokens(head) {
        if cpp_type_intro_keyword(token.text) {
            return true;
        }
        if cpp_declaration_prefix_token(token.text) {
            continue;
        }
        return false;
    }

    false
}

fn decorated_declaration_head_has_type_prefix(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    let head = text
        .split(['{', ';'])
        .next()
        .unwrap_or(text.as_str())
        .trim();
    let mut saw_prefix = false;
    for token in cpp_head_tokens(head) {
        if cpp_type_intro_keyword(token.text) {
            return saw_prefix;
        }
        if cpp_declaration_prefix_token(token.text) {
            saw_prefix = true;
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
    let tokens = cpp_head_tokens(head);
    for (index, token) in tokens.iter().enumerate() {
        let kind = match token.text {
            "class" => "class",
            "struct" | "union" | "enum" => "type",
            _ => continue,
        };
        let name = cpp_type_name_after_intro(head, &tokens[index + 1..])?;
        return Some((name.to_owned(), kind, syntax_range(node)));
    }

    None
}

#[derive(Clone, Copy)]
struct CppHeadToken<'text> {
    text: &'text str,
    start: usize,
    end: usize,
}

fn cpp_head_tokens(head: &str) -> Vec<CppHeadToken<'_>> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    for (index, character) in head.char_indices() {
        if character.is_ascii_alphanumeric() || character == '_' {
            token_start.get_or_insert(index);
            continue;
        }
        if let Some(start) = token_start.take() {
            tokens.push(CppHeadToken {
                text: &head[start..index],
                start,
                end: index,
            });
        }
    }
    if let Some(start) = token_start {
        tokens.push(CppHeadToken {
            text: &head[start..],
            start,
            end: head.len(),
        });
    }

    tokens
}

fn cpp_type_name_after_intro<'text>(
    head: &'text str,
    tokens: &[CppHeadToken<'text>],
) -> Option<&'text str> {
    let mut index = cpp_skip_type_name_prefix(tokens);
    while tokens
        .get(index)
        .is_some_and(|token| matches!(token.text, "class" | "struct"))
    {
        index += 1;
    }

    let mut name = *tokens.get(index)?;
    if !cpp_type_name_candidate(name.text) {
        return None;
    }

    while let Some(next) = tokens.get(index + 1) {
        if !cpp_type_name_candidate(next.text) || !cpp_tokens_joined_by_qualifier(head, name, *next)
        {
            break;
        }
        name = *next;
        index += 1;
    }

    Some(name.text)
}

fn cpp_skip_type_name_prefix(tokens: &[CppHeadToken<'_>]) -> usize {
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        if cpp_type_name_decorator_prefix(token.text) {
            index += 1;
            while tokens
                .get(index)
                .is_some_and(|payload| cpp_decorator_payload_token(payload.text))
            {
                index += 1;
            }
            continue;
        }
        if cpp_decorator_payload_token(token.text) {
            index += 1;
            continue;
        }
        break;
    }

    index
}

fn cpp_tokens_joined_by_qualifier(
    head: &str,
    left: CppHeadToken<'_>,
    right: CppHeadToken<'_>,
) -> bool {
    let separator = &head[left.end..right.start];
    separator.contains("::")
        && separator
            .chars()
            .all(|character| character == ':' || character.is_ascii_whitespace())
}

fn cpp_type_name_candidate(token: &str) -> bool {
    if cpp_keyword_token(token) {
        return false;
    }
    if cpp_builtin_type_token(token) {
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
        || cpp_decorator_payload_token(token)
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

fn cpp_type_name_decorator_prefix(token: &str) -> bool {
    token.starts_with("__")
        || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
}

fn cpp_decorator_payload_token(token: &str) -> bool {
    matches!(
        token,
        "dllimport" | "dllexport" | "visibility" | "default" | "hidden"
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

fn cpp_builtin_type_token(token: &str) -> bool {
    matches!(
        token,
        "auto"
            | "bool"
            | "char"
            | "char8_t"
            | "char16_t"
            | "char32_t"
            | "double"
            | "float"
            | "int"
            | "long"
            | "short"
            | "signed"
            | "unsigned"
            | "void"
            | "wchar_t"
    )
}

fn cpp_decorator_token(token: &str) -> bool {
    token.starts_with("__")
        || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
        || (token.chars().any(|character| character == '_')
            && token.chars().all(|character| {
                character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
            }))
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
