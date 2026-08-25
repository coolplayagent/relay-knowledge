//! Import query-path classification and target/source path context.

use super::{super::relevance::query_is_single_symbol_identity, binding_terms::camel_case_terms};

const MIN_IMPORT_COVERAGE_TERM_LEN: usize = 3;

pub(super) fn query_looks_like_import_path(query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.contains('/') || trimmed.contains('\\') || query_contains_file_extension(trimmed)
}

pub(super) fn import_path_lookup_token(request_query: &str) -> Option<&str> {
    if !query_looks_like_import_path(request_query) {
        return None;
    }
    let path_token = request_query
        .split_whitespace()
        .map(import_path_token)
        .find(|token| query_looks_like_import_path(token))?;
    (!path_token.is_empty()).then_some(path_token)
}

pub(in super::super) fn import_target_symbol_query(query: &str) -> Option<&str> {
    let trimmed = query.trim();
    if standalone_import_target_symbol(trimmed) {
        return Some(trimmed);
    }
    if !query_looks_like_import_path(trimmed) {
        return None;
    }

    let mut candidates = trimmed
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| {
                !(character.is_ascii_alphanumeric() || character == '_')
            })
        })
        .filter(|token| standalone_import_target_symbol(token))
        .filter(|token| {
            token
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_uppercase())
        });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(super) fn import_path_token_matches_target_hint(path_token: &str, target_hint: &str) -> bool {
    let normalized_path = normalized_import_path(path_token);
    let normalized_target = normalized_import_path(target_hint);
    !normalized_path.is_empty()
        && (normalized_target == normalized_path
            || normalized_target.ends_with(&format!("/{normalized_path}")))
}

fn standalone_import_target_symbol(token: &str) -> bool {
    !token.is_empty()
        && query_is_single_symbol_identity(token)
        && !token.contains('/')
        && !token.contains('\\')
        && !token.contains('.')
        && !token.contains("::")
        && !query_contains_file_extension(token)
}

fn import_path_token(token: &str) -> &str {
    token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric()
            || matches!(character, '_' | '-' | '.' | '/' | '\\' | '@'))
    })
}

fn normalized_import_path(path: &str) -> String {
    path.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric()
            || matches!(character, '_' | '-' | '.' | '/' | '\\' | '@'))
    })
    .chars()
    .map(|character| match character {
        '.' | '\\' => '/',
        other => other.to_ascii_lowercase(),
    })
    .collect::<String>()
    .split('/')
    .filter(|component| !component.is_empty())
    .collect::<Vec<_>>()
    .join("/")
}

pub(super) fn query_contains_file_extension(query: &str) -> bool {
    query.split_whitespace().any(|term| {
        let term = term.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
        });
        let Some((stem, extension)) = term.rsplit_once('.') else {
            return false;
        };
        !stem.is_empty() && file_extension_is_path_like(extension)
    })
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

pub(super) fn parent_dir(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

pub(super) fn path_has_header_extension(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .and_then(|file_name| file_name.rsplit_once('.').map(|(_, extension)| extension))
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "h" | "hh" | "hpp" | "hxx" | "inc" | "ipp"
            )
        })
}

pub(super) fn target_stem_terms(query: &str, target_hint: Option<&str>) -> Vec<String> {
    target_stem(query, target_hint)
        .map(|stem| stem_terms(&stem))
        .unwrap_or_default()
}

pub(super) fn target_stem(query: &str, target_hint: Option<&str>) -> Option<String> {
    let target = target_hint
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .unwrap_or(query);
    let file_name = target
        .trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-' | '.' | '/' | '\\'))
        })
        .rsplit(['/', '\\'])
        .next()?;
    let stem = file_stem(file_name);
    (!stem.is_empty()).then(|| stem.to_ascii_lowercase())
}

pub(super) fn file_stem(file_name: &str) -> &str {
    file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem)
}

pub(super) fn stem_terms(stem: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for token in stem
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        for term in camel_case_terms(token) {
            if term.len() >= MIN_IMPORT_COVERAGE_TERM_LEN {
                terms.push(term);
            }
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

pub(super) fn source_file_can_implement_header(file_name: &str) -> bool {
    file_name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| matches!(extension, "c" | "cc" | "cpp" | "cxx" | "m" | "mm"))
}

pub(super) fn import_target_mentions_query(
    module: &str,
    target_hint: Option<&str>,
    query: &str,
) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return false;
    }
    let query = query.to_ascii_lowercase();
    [module, target_hint.unwrap_or_default()]
        .into_iter()
        .map(str::to_ascii_lowercase)
        .any(|field| field.trim() == query || field.contains(&query))
}

#[cfg(test)]
#[path = "path_context_tests.rs"]
mod tests;
