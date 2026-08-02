use std::path::Path;

use super::super::{languages::language_id, parser::dependency_manifest_language_ids};

pub(in crate::code) fn source_language_filter_allows(path: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    if language_id(path).is_some_and(|language| {
        filters.iter().any(|filter| {
            filter == language
                || c_cpp_header_only_filter_allows(path, language, filter)
                || cxx_header_filter_allows(path, language, filter)
                || unknown_filter_allows_document_path(path, language, filter)
        })
    }) {
        return true;
    }
    dependency_manifest_language_ids(path).is_some_and(|languages| {
        languages
            .iter()
            .any(|language| filters.iter().any(|filter| filter == language))
    })
}

fn cxx_header_filter_allows(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "cpp" && language_id == "c" && path.to_ascii_lowercase().ends_with(".h")
}

fn c_cpp_header_only_filter_allows(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "__relay_c_cpp_header_only__"
        && matches!(language_id, "c" | "cpp")
        && c_cpp_header_path(path)
}

fn c_cpp_header_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("h" | "hh" | "hpp" | "hxx")
    )
}

fn unknown_filter_allows_document_path(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "unknown" && document_like_language_path(path, language_id)
}

fn document_like_language_path(path: &str, language_id: &str) -> bool {
    matches!(
        language_id,
        "markdown" | "json" | "yaml" | "toml" | "xml" | "ini" | "properties"
    ) || matches!(
        path.rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("md" | "markdown" | "txt" | "rst" | "adoc")
    )
}

#[cfg(test)]
#[path = "language_scope_tests.rs"]
mod tests;
