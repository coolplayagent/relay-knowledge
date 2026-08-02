//! Normalizes query text into reusable identity, scoring, and FTS terms.

use super::super::identifiers::identifier_terms_equivalent;

pub(in crate::storage::sqlite::code::query) fn identity_terms(token: &str) -> Vec<String> {
    token
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(in crate::storage::sqlite::code::query) fn simple_identity_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

const MIN_DECOMPOSED_SCORE_TERM_LEN: usize = 2;

pub(in crate::storage::sqlite::code::query) fn score_query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.is_empty() {
            continue;
        }
        push_score_query_token(&mut tokens, raw_token.to_ascii_lowercase());
        if !raw_score_token_allows_decomposition(raw_token) {
            continue;
        }
        for term in raw_token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|term| term.len() >= MIN_DECOMPOSED_SCORE_TERM_LEN)
        {
            push_score_query_token(&mut tokens, term.to_ascii_lowercase());
        }
    }

    tokens
}

fn raw_score_token_allows_decomposition(token: &str) -> bool {
    !(token.contains('/') || token.contains('\\') || token_has_path_like_extension(token))
}

pub(in crate::storage::sqlite::code::query) fn token_has_path_like_extension(token: &str) -> bool {
    let token = token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    });
    let Some((stem, extension)) = token.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty() && file_extension_is_path_like(extension)
}

fn file_extension_is_path_like(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "gradle"
            | "h"
            | "hh"
            | "hpp"
            | "hxx"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "md"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "swift"
            | "ts"
            | "tsx"
            | "txt"
            | "xml"
            | "yaml"
            | "yml"
    )
}

fn push_score_query_token(tokens: &mut Vec<String>, token: String) {
    if !token.is_empty() && !tokens.contains(&token) {
        tokens.push(token);
    }
}

pub(in crate::storage::sqlite::code::query) fn identifier_field_matches_token(
    field: &str,
    token: &str,
) -> bool {
    identifier_tokens(field).any(|candidate| {
        identifier_terms_equivalent(candidate, token)
            || candidate
                .split('_')
                .filter(|part| !part.is_empty())
                .any(|part| identifier_terms_equivalent(part, token))
            || camel_case_terms(candidate)
                .iter()
                .any(|part| identifier_terms_equivalent(part, token))
    })
}

pub(in crate::storage::sqlite::code::query) fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(in crate::storage::sqlite::code::query) fn identifier_search_tokens(
    value: &str,
) -> Vec<String> {
    let mut tokens = identifier_match_terms(value);
    tokens.sort();
    tokens.dedup();

    tokens
}

pub(in crate::storage::sqlite::code::query) fn identifier_match_terms(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for token in identifier_tokens(value) {
        tokens.push(token.to_ascii_lowercase());
        tokens.extend(
            token
                .split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        tokens.extend(camel_case_terms(token));
    }

    tokens
}

pub(in crate::storage::sqlite::code::query) fn identifier_tokens(
    value: &str,
) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
}

fn camel_case_terms(token: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let chars = token.char_indices().collect::<Vec<_>>();
    for (index, (byte_index, character)) in chars.iter().enumerate() {
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if *byte_index > start && starts_upper_word {
            terms.push(token[start..*byte_index].to_ascii_lowercase());
            start = *byte_index;
        }
        previous = Some(*character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }

    terms
}

pub(in crate::storage::sqlite::code::query) fn escape_sql_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }

    escaped
}
