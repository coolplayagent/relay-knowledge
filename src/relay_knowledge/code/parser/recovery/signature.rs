use super::{
    declaration::{
        CFamilyHeadToken, c_declaration_qualifier_token, c_family_builtin_type_token,
        c_family_decorator_payload_token, c_family_head_tokens, c_family_known_decorator_token,
        c_family_parenthesized_prefix_end, c_family_token_starts_in_decorator_payload,
        c_family_typedef_declaration_head, c_identifier_char, c_identifier_name,
    },
    scan::{
        code_contains_char, line_has_balanced_delimiters, parameter_list_has_empty_slot,
        scan_code_line_indices,
    },
};

#[cfg(test)]
#[path = "signature_tests.rs"]
mod tests;

pub(in crate::code::parser) fn decorated_function_head_has_recoverable_tail(
    head: &str,
    allow_default_arguments: bool,
    allow_cpp_method_suffix: bool,
    allow_operator_declarator: bool,
) -> bool {
    c_family_function_signature_tail_is_recoverable(
        head,
        allow_default_arguments,
        allow_cpp_method_suffix,
        allow_operator_declarator,
    )
}

pub(in crate::code::parser) fn c_family_typedef_like_function_signature(trimmed: &str) -> bool {
    c_family_typedef_like_function_signature_with_options(trimmed, false, true, false)
}

pub(super) fn c_family_typedef_like_function_signature_with_options(
    trimmed: &str,
    allow_default_arguments: bool,
    allow_cpp_method_suffix: bool,
    allow_operator_declarator: bool,
) -> bool {
    if !line_has_balanced_delimiters(trimmed) {
        return false;
    }
    let Some(parameter_start) =
        c_family_top_level_parameter_start(trimmed, allow_operator_declarator)
    else {
        return false;
    };
    let head = &trimmed[..parameter_start];
    if code_contains_char(head, '=') && !c_family_operator_before_open(trimmed, parameter_start) {
        return false;
    }
    if !c_family_function_signature_tail_is_recoverable(
        trimmed,
        allow_default_arguments,
        allow_cpp_method_suffix,
        allow_operator_declarator,
    ) {
        return false;
    }

    c_family_typedef_declaration_head(head)
}

fn c_family_function_signature_tail_is_recoverable(
    trimmed: &str,
    allow_default_arguments: bool,
    allow_cpp_method_suffix: bool,
    allow_operator_declarator: bool,
) -> bool {
    if !line_has_balanced_delimiters(trimmed) {
        return false;
    }
    let Some(parameter_start) =
        c_family_top_level_parameter_start(trimmed, allow_operator_declarator)
    else {
        return false;
    };
    let Some(parameter_end) = c_family_closing_parenthesis_index(&trimmed[parameter_start..])
    else {
        return false;
    };
    let parameter_text = &trimmed[parameter_start + 1..parameter_start + parameter_end];
    if parameter_list_has_empty_slot(parameter_text) {
        return false;
    }
    if !allow_default_arguments && code_contains_char(parameter_text, '=') {
        return false;
    }
    let tail = trimmed[parameter_start + parameter_end + 1..].trim();
    c_family_typedef_signature_tail_is_declaration_shaped(tail, allow_cpp_method_suffix)
}

pub(super) fn c_family_top_level_parameter_start(
    text: &str,
    allow_operator_declarator: bool,
) -> Option<usize> {
    let mut depth = 0usize;
    let mut candidate = None;
    let literals_closed = scan_code_line_indices(text, |index, character| match character {
        '(' => {
            if depth == 0
                && c_family_parameter_open_looks_like_declarator(
                    text,
                    index,
                    allow_operator_declarator,
                )
            {
                candidate = Some(index);
            }
            depth += 1;
        }
        ')' => depth = depth.saturating_sub(1),
        _ => {}
    });

    literals_closed.then_some(candidate).flatten()
}

fn c_family_parameter_open_looks_like_declarator(
    text: &str,
    parameter_start: usize,
    allow_operator_declarator: bool,
) -> bool {
    c_identifier_before_open(text, parameter_start).is_some_and(|token| {
        c_identifier_name(token.text)
            && !c_declaration_qualifier_token(token.text)
            && !c_family_known_decorator_token(token.text)
            && !c_family_decorator_payload_token(token.text)
            && !c_family_token_starts_in_decorator_payload(text, token.start)
    }) || (allow_operator_declarator && c_family_operator_before_open(text, parameter_start))
}

fn c_identifier_before_open(text: &str, parameter_start: usize) -> Option<CFamilyHeadToken<'_>> {
    let name_end = text[..parameter_start].trim_end().len();
    let name_start = text[..name_end]
        .char_indices()
        .rev()
        .find(|(_, character)| !c_identifier_char(*character))
        .map_or(0, |(index, character)| index + character.len_utf8());
    (name_start < name_end).then_some(CFamilyHeadToken {
        text: &text[name_start..name_end],
        start: name_start,
        end: name_end,
    })
}

fn c_family_operator_before_open(text: &str, parameter_start: usize) -> bool {
    let prefix = &text[..text[..parameter_start].trim_end().len()];
    let Some(operator_start) = prefix.rfind("operator") else {
        return false;
    };
    if prefix[..operator_start]
        .chars()
        .next_back()
        .is_some_and(c_identifier_char)
    {
        return false;
    }
    let suffix = prefix[operator_start + "operator".len()..].trim();
    c_family_punctuation_operator_suffix(suffix) || c_family_conversion_operator_suffix(suffix)
}

fn c_family_punctuation_operator_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix
            .chars()
            .all(|character| character.is_ascii_punctuation() || character.is_ascii_whitespace())
}

fn c_family_conversion_operator_suffix(suffix: &str) -> bool {
    let tokens = c_family_head_tokens(suffix);
    !tokens.is_empty()
        && tokens.iter().all(|token| {
            c_declaration_qualifier_token(token.text)
                || c_identifier_name(token.text)
                || c_family_builtin_type_token(token.text)
        })
        && suffix.chars().all(|character| {
            c_identifier_char(character)
                || character.is_ascii_whitespace()
                || matches!(character, ':' | '*' | '&' | '<' | '>' | ',')
        })
}

fn c_family_typedef_signature_tail_is_declaration_shaped(
    tail: &str,
    allow_cpp_method_suffix: bool,
) -> bool {
    tail.is_empty()
        || matches!(tail, ";" | "{")
        || (allow_cpp_method_suffix && c_family_cpp_method_suffix_tail(tail))
        || c_family_postfix_attribute_tail(tail)
}

pub(in crate::code::parser) fn decorated_function_head_has_recovery_decorator(head: &str) -> bool {
    let Some(parameter_start) = c_family_top_level_parameter_start(head, true) else {
        return false;
    };
    let parameter_end = c_family_closing_parenthesis_index(&head[parameter_start..])
        .map_or(head.len(), |index| parameter_start + index + 1);
    let prefix = &head[..parameter_start];
    let suffix = &head[parameter_end..];
    c_family_head_tokens(prefix)
        .iter()
        .chain(c_family_head_tokens(suffix).iter())
        .any(|token| {
            c_family_known_decorator_token(token.text)
                || matches!(
                    token.text,
                    "__always_inline"
                        | "__inline"
                        | "__inline__"
                        | "__declspec"
                        | "__declspec__"
                        | "__attribute"
                        | "__attribute__"
                        | "attribute"
                )
        })
}

fn c_family_postfix_attribute_tail(mut tail: &str) -> bool {
    let mut consumed_attribute = false;
    loop {
        tail = tail.trim_start();
        if tail.is_empty() || matches!(tail, ";" | "{") {
            return consumed_attribute;
        }
        let Some((token, token_end)) = c_family_leading_identifier(tail) else {
            return false;
        };
        if c_family_known_decorator_token(token) {
            let after_token = tail[token_end..].trim_start();
            let Some(payload_end) = c_family_parenthesized_prefix_end(after_token) else {
                return false;
            };
            tail = &after_token[payload_end..];
            consumed_attribute = true;
            continue;
        }
        if matches!(
            token,
            "const" | "final" | "noexcept" | "override" | "volatile"
        ) {
            tail = &tail[token_end..];
            if token == "noexcept" {
                let trimmed = tail.trim_start();
                if let Some(payload_end) = c_family_parenthesized_prefix_end(trimmed) {
                    tail = &trimmed[payload_end..];
                }
            }
            continue;
        }
        return false;
    }
}

fn c_family_leading_identifier(text: &str) -> Option<(&str, usize)> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, character) in chars {
        if !c_identifier_char(character) {
            return Some((&text[..end], index));
        }
        end = index + character.len_utf8();
    }
    Some((text, text.len()))
}

fn c_family_cpp_method_suffix_tail(tail: &str) -> bool {
    let mut tail = tail.trim();
    if let Some(stripped) = tail.strip_suffix("= 0;") {
        tail = stripped.trim_end();
    } else if let Some(stripped) = tail.strip_suffix("=0;") {
        tail = stripped.trim_end();
    } else if let Some(stripped) = tail.strip_suffix("= 0") {
        tail = stripped.trim_end();
    } else if let Some(stripped) = tail.strip_suffix("=0") {
        tail = stripped.trim_end();
    } else if let Some(stripped) = tail.strip_suffix('{').or_else(|| tail.strip_suffix(';')) {
        tail = stripped.trim_end();
    }
    if tail.is_empty() || !line_has_balanced_delimiters(tail) {
        return false;
    }
    let mut consumed_suffix = false;
    loop {
        tail = tail.trim_start();
        if tail.is_empty() {
            return consumed_suffix;
        }
        let Some((token, token_end)) = c_family_leading_identifier(tail) else {
            return false;
        };
        if !matches!(
            token,
            "const" | "final" | "noexcept" | "override" | "volatile"
        ) {
            return false;
        }
        tail = &tail[token_end..];
        if token == "noexcept" {
            let trimmed = tail.trim_start();
            if let Some(payload_end) = c_family_parenthesized_prefix_end(trimmed) {
                tail = &trimmed[payload_end..];
            }
        }
        consumed_suffix = true;
    }
}

fn c_family_closing_parenthesis_index(text: &str) -> Option<usize> {
    if !text.starts_with('(') {
        return None;
    }
    let mut depth = 0isize;
    let mut matched_end = None;
    let literals_closed = scan_code_line_indices(text, |index, character| {
        if matched_end.is_some() {
            return;
        }
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    matched_end = Some(index);
                }
            }
            _ => {}
        }
    });
    literals_closed.then_some(matched_end).flatten()
}
