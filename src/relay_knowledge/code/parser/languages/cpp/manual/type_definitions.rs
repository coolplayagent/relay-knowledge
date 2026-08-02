use tree_sitter::Node;

use super::super::super::super::nodes::{SyntaxRange, node_text, syntax_range};
use super::lexical::{
    CppHeadToken, cpp_declaration_prefix_token, cpp_decorator_payload_token, cpp_head_tokens,
    cpp_tokens_joined_by_qualifier, cpp_type_intro_keyword, cpp_type_name_candidate,
    cpp_type_name_decorator_prefix,
};

pub(super) fn decorated_cpp_declaration_type_symbol(
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

pub(super) fn cpp_type_declaration_context(content: &str, node: Node<'_>) -> bool {
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

pub(super) fn decorated_declaration_head_starts_with_type_definition(
    content: &str,
    node: Node<'_>,
) -> bool {
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

pub(super) fn decorated_declaration_head_declares_function(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    let head = text
        .split(['{', ';'])
        .next()
        .unwrap_or(text.as_str())
        .trim();
    let tokens = cpp_head_tokens(head);
    for (index, token) in tokens.iter().enumerate() {
        if !cpp_type_intro_keyword(token.text) {
            continue;
        }
        let Some(name) = cpp_type_name_after_intro_token(head, &tokens[index + 1..]) else {
            return false;
        };
        return head[name.end..].contains('(');
    }

    false
}

pub(super) fn decorated_cpp_type_symbol(
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

fn cpp_type_name_after_intro<'text>(
    head: &'text str,
    tokens: &[CppHeadToken<'text>],
) -> Option<&'text str> {
    cpp_type_name_after_intro_token(head, tokens).map(|token| token.text)
}

fn cpp_type_name_after_intro_token<'text>(
    head: &'text str,
    tokens: &[CppHeadToken<'text>],
) -> Option<CppHeadToken<'text>> {
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

    Some(name)
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

#[cfg(test)]
#[path = "type_definitions_tests.rs"]
mod tests;
