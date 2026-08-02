//! Import query-path classification and target/source path context.

use super::binding_terms::camel_case_terms;

const MIN_IMPORT_COVERAGE_TERM_LEN: usize = 3;

pub(super) fn query_looks_like_import_path(query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.contains('/') || trimmed.contains('\\') || query_contains_file_extension(trimmed)
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
