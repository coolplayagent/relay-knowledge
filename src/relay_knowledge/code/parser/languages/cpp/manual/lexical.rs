#[derive(Clone, Copy)]
pub(super) struct CppHeadToken<'text> {
    pub(super) text: &'text str,
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) fn cpp_head_tokens(head: &str) -> Vec<CppHeadToken<'_>> {
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

pub(super) fn cpp_tokens_joined_by_qualifier(
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

pub(super) fn cpp_type_name_candidate(token: &str) -> bool {
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

pub(super) fn cpp_type_intro_keyword(token: &str) -> bool {
    matches!(token, "class" | "struct" | "union" | "enum")
}

pub(super) fn cpp_declaration_prefix_token(token: &str) -> bool {
    cpp_decorator_token(token)
        || cpp_decorator_payload_token(token)
        || matches!(
            token,
            "__always_inline"
                | "__inline"
                | "__inline__"
                | "alignas"
                | "const"
                | "constexpr"
                | "export"
                | "extern"
                | "friend"
                | "inline"
                | "static"
                | "template"
                | "typename"
                | "using"
                | "volatile"
        )
}

pub(super) fn cpp_type_name_decorator_prefix(token: &str) -> bool {
    cpp_double_underscore_decorator_token(token)
        || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
}

pub(super) fn cpp_decorator_payload_token(token: &str) -> bool {
    matches!(
        token,
        "always_inline"
            | "annotate"
            | "dllimport"
            | "dllexport"
            | "visibility"
            | "default"
            | "hidden"
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

pub(super) fn cpp_builtin_type_token(token: &str) -> bool {
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

pub(super) fn cpp_decorator_token(token: &str) -> bool {
    cpp_double_underscore_decorator_token(token)
        || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
        || (token.chars().any(|character| character == '_')
            && token.chars().all(|character| {
                character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
            }))
}

fn cpp_double_underscore_decorator_token(token: &str) -> bool {
    matches!(
        token,
        "__attribute__" | "__attribute" | "__declspec" | "__declspec__" | "attribute"
    )
}

#[cfg(test)]
#[path = "lexical_tests.rs"]
mod tests;
