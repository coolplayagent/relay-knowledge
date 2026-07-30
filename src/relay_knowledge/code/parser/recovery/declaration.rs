use super::scan::token_starts_in_angle_arguments;

#[cfg(test)]
#[path = "declaration_tests.rs"]
mod tests;
pub(super) fn c_family_typedef_like_initializer_declaration(trimmed: &str) -> bool {
    let Some((head, initializer)) = trimmed.split_once('=') else {
        return false;
    };
    let initializer = initializer.trim_start();
    if !initializer.starts_with('{') {
        return false;
    }

    c_family_typedef_declaration_head(head)
}

pub(super) fn c_family_typedef_declaration_head(head: &str) -> bool {
    let normalized;
    let head = if let Some(stripped) = c_family_head_without_declarator_scope(head) {
        normalized = stripped;
        normalized.as_str()
    } else {
        head
    };
    let tokens = c_family_head_tokens(head);
    let name = tokens
        .last()
        .copied()
        .filter(|token| c_identifier_name(token.text));
    let Some(name) = name else {
        return false;
    };
    if c_family_builtin_type_token(name.text)
        || (name.text.ends_with("_t") && c_identifier_name(name.text))
    {
        return false;
    }
    let Some(type_index) = tokens[..tokens.len().saturating_sub(1)]
        .iter()
        .rposition(|token| {
            !c_declaration_qualifier_token(token.text)
                && !token_starts_in_angle_arguments(head, token.start)
        })
    else {
        return false;
    };
    if !c_family_typedef_like_type_at(head, &tokens, type_index) {
        return false;
    }
    let type_start = c_family_qualified_type_start(head, &tokens, type_index);
    if !tokens[..type_start].iter().all(|token| {
        c_declaration_qualifier_token(token.text)
            || c_family_typedef_like_type_token(token.text)
            || c_family_decorator_payload_token(token.text)
            || c_family_token_starts_in_decorator_payload(head, token.start)
    }) {
        return false;
    }

    true
}

pub(super) fn c_family_parenthesized_prefix_end(text: &str) -> Option<usize> {
    if !text.starts_with('(') {
        return None;
    }
    let mut depth = 0isize;
    let mut matched_end = None;
    let literals_closed = super::scan::scan_code_line_indices(text, |index, character| {
        if matched_end.is_some() {
            return;
        }
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    matched_end = Some(index + character.len_utf8());
                }
            }
            _ => {}
        }
    });
    literals_closed.then_some(matched_end).flatten()
}

pub(super) fn c_family_token_starts_in_decorator_payload(head: &str, token_start: usize) -> bool {
    c_family_head_tokens(head)
        .iter()
        .filter(|token| c_family_known_decorator_token(token.text))
        .any(|decorator| {
            let after_decorator = &head[decorator.end..];
            let leading_whitespace = after_decorator
                .char_indices()
                .find(|(_, character)| !character.is_ascii_whitespace())
                .map_or(after_decorator.len(), |(index, _)| index);
            let payload_start = decorator.end + leading_whitespace;
            if payload_start >= token_start {
                return false;
            }
            c_family_parenthesized_prefix_end(&head[payload_start..])
                .is_some_and(|payload_len| token_start < payload_start + payload_len)
        })
}

fn c_family_head_without_declarator_scope(head: &str) -> Option<String> {
    if let Some(operator_start) = head.rfind("operator") {
        let stripped_prefix =
            c_family_strip_trailing_qualified_declarator_scope(&head[..operator_start]);
        return (stripped_prefix.len() < operator_start)
            .then(|| format!("{stripped_prefix}operator"));
    }
    let name_end = head.trim_end().len();
    let name_start = head[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !c_identifier_char(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    if name_start >= name_end {
        return None;
    }
    let prefix = &head[..name_start];
    let stripped_prefix = c_family_strip_trailing_qualified_declarator_scope(prefix);
    (stripped_prefix.len() < prefix.len())
        .then(|| format!("{stripped_prefix}{}", &head[name_start..name_end]))
}

#[derive(Clone, Copy)]
pub(super) struct CFamilyHeadToken<'text> {
    pub(super) text: &'text str,
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) fn c_family_head_tokens(head: &str) -> Vec<CFamilyHeadToken<'_>> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    for (index, character) in head.char_indices() {
        if c_identifier_char(character) {
            token_start.get_or_insert(index);
            continue;
        }
        if let Some(start) = token_start.take() {
            tokens.push(CFamilyHeadToken {
                text: &head[start..index],
                start,
                end: index,
            });
        }
    }
    if let Some(start) = token_start {
        tokens.push(CFamilyHeadToken {
            text: &head[start..],
            start,
            end: head.len(),
        });
    }

    tokens
}

fn c_family_strip_trailing_qualified_declarator_scope(prefix: &str) -> &str {
    let mut cursor = prefix.trim_end().len();
    let mut stripped_scope = false;
    loop {
        let before_colons = prefix[..cursor].trim_end().len();
        if !prefix[..before_colons].ends_with("::") {
            break;
        }
        let scope_end = before_colons.saturating_sub(2);
        let ident_end = prefix[..scope_end].trim_end().len();
        let ident_start = prefix[..ident_end]
            .char_indices()
            .rev()
            .find(|(_, character)| !c_identifier_char(*character))
            .map_or(0, |(index, character)| index + character.len_utf8());
        if ident_start >= ident_end {
            break;
        }
        cursor = ident_start;
        stripped_scope = true;
    }

    if stripped_scope {
        &prefix[..cursor]
    } else {
        prefix
    }
}

fn c_family_typedef_like_type_at(
    head: &str,
    tokens: &[CFamilyHeadToken<'_>],
    type_index: usize,
) -> bool {
    let token = tokens[type_index].text;
    if c_family_typedef_like_type_token(token) {
        return true;
    }
    c_family_qualified_type_start(head, tokens, type_index) < type_index
        && c_identifier_name(token)
        && !c_family_builtin_type_token(token)
}

fn c_family_qualified_type_start(
    head: &str,
    tokens: &[CFamilyHeadToken<'_>],
    type_index: usize,
) -> usize {
    let mut start = type_index;
    while start > 0
        && c_identifier_name(tokens[start - 1].text)
        && c_family_tokens_joined_by_qualifier(head, tokens[start - 1], tokens[start])
    {
        start -= 1;
    }
    start
}

fn c_family_tokens_joined_by_qualifier(
    head: &str,
    left: CFamilyHeadToken<'_>,
    right: CFamilyHeadToken<'_>,
) -> bool {
    let separator = &head[left.end..right.start];
    separator.contains("::")
        && separator
            .chars()
            .all(|character| character == ':' || character.is_ascii_whitespace())
}

fn c_family_typedef_like_type_token(token: &str) -> bool {
    c_family_builtin_type_token(token)
        || c_family_tag_type_keyword(token)
        || (token.ends_with("_t") && c_identifier_name(token))
        || c_family_external_type_token(token)
}

pub(super) fn c_family_builtin_type_token(token: &str) -> bool {
    matches!(
        token,
        "bool"
            | "char"
            | "double"
            | "float"
            | "int"
            | "long"
            | "short"
            | "signed"
            | "unsigned"
            | "void"
    )
}

fn c_family_tag_type_keyword(token: &str) -> bool {
    matches!(token, "enum" | "struct" | "union")
}

pub(super) fn c_family_external_type_token(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
        && token
            .chars()
            .any(|character| character.is_ascii_lowercase())
        && c_identifier_name(token)
}

pub(super) fn c_declaration_qualifier_token(token: &str) -> bool {
    matches!(
        token,
        "__always_inline"
            | "__attribute__"
            | "__attribute"
            | "__declspec"
            | "__declspec__"
            | "__inline"
            | "__inline__"
            | "always_inline"
            | "attribute"
            | "const"
            | "extern"
            | "inline"
            | "register"
            | "restrict"
            | "static"
            | "volatile"
    )
}

pub(super) fn c_family_known_decorator_token(token: &str) -> bool {
    matches!(
        token,
        "__attribute__" | "__attribute" | "__declspec" | "__declspec__" | "attribute"
    ) || token.ends_with("_API")
        || token.ends_with("_EXPORT")
        || token.ends_with("_EXPORTS")
}

pub(super) fn c_family_decorator_payload_token(token: &str) -> bool {
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

pub(super) fn c_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

pub(super) fn c_identifier_name(token: &str) -> bool {
    let mut characters = token.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(c_identifier_char)
}
