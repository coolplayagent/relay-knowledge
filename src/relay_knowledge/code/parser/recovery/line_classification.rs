use super::{
    c_family_decorator_payload_token, c_family_external_type_token, c_family_head_tokens,
    c_family_known_decorator_token, c_family_top_level_parameter_start,
    c_family_typedef_like_function_signature, c_identifier_name,
    decorated_function_head_has_recovery_decorator,
};

pub(super) fn c_family_decorated_type_line(trimmed: &str) -> bool {
    let tokens = trimmed
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.iter().enumerate().any(|(index, token)| {
        matches!(*token, "class" | "struct" | "enum" | "union")
            && (c_family_decorator_before_type(&tokens, index)
                || c_family_decorator_after_type(&tokens, index))
    })
}

fn c_family_decorator_before_type(tokens: &[&str], index: usize) -> bool {
    let mut cursor = index;
    while let Some(previous_index) = cursor.checked_sub(1) {
        let Some(previous) = tokens.get(previous_index) else {
            return false;
        };
        if c_family_known_decorator_token(previous) {
            return true;
        }
        if !c_family_decorator_payload_token(previous) && !c_identifier_name(previous) {
            return false;
        }
        cursor = previous_index;
    }

    false
}

fn c_family_decorator_after_type(tokens: &[&str], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|candidate| c_family_known_decorator_token(candidate))
}

pub(super) fn c_family_macro_name(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
        && token.chars().any(|character| character == '_')
        && token
            .chars()
            .any(|character| character.is_ascii_uppercase())
}

pub(super) fn c_family_recoverable_error_function_signature(trimmed: &str) -> bool {
    if !c_family_typedef_like_function_signature(trimmed) {
        return false;
    }
    let Some(parameter_start) = c_family_top_level_parameter_start(trimmed, false) else {
        return false;
    };
    let head = &trimmed[..parameter_start];
    let tokens = c_family_head_tokens(head);
    decorated_function_head_has_recovery_decorator(trimmed)
        || tokens
            .iter()
            .take(tokens.len().saturating_sub(1))
            .any(|token| c_family_recoverable_error_type_token(token.text))
}

fn c_family_recoverable_error_type_token(token: &str) -> bool {
    c_family_external_type_token(token) || (token.ends_with("_t") && c_identifier_name(token))
}
