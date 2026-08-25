use std::collections::BTreeSet;

use super::super::{generated_detection, languages::language_id, safe_git_blob_path};
use super::SourceGrepRequest;

pub(crate) const SOURCE_GREP_CANDIDATE_FILE_LIMIT: usize = 256;
const SOURCE_GREP_MATCHES_PER_CANDIDATE_FILE: usize = 2;
const SOURCE_GREP_CANDIDATE_MATCH_LIMIT: usize =
    SOURCE_GREP_CANDIDATE_FILE_LIMIT * SOURCE_GREP_MATCHES_PER_CANDIDATE_FILE;

pub(crate) fn bounded_source_grep_candidate_match_limit(
    result_limit: usize,
    candidate_path_count: usize,
) -> usize {
    let fair_candidate_limit = candidate_path_count
        .min(SOURCE_GREP_CANDIDATE_FILE_LIMIT)
        .saturating_mul(SOURCE_GREP_MATCHES_PER_CANDIDATE_FILE);

    result_limit
        .max(fair_candidate_limit)
        .min(SOURCE_GREP_CANDIDATE_MATCH_LIMIT)
}

pub(super) struct CandidatePaths {
    pub(super) paths: Vec<String>,
    pub(super) degraded_reason: Option<String>,
}

pub(super) fn selected_candidate_paths(request: &SourceGrepRequest) -> CandidatePaths {
    let mut paths = Vec::new();
    let mut seen = BTreeSet::new();
    let mut exhausted = false;
    for path in &request.paths {
        if paths.len() >= SOURCE_GREP_CANDIDATE_FILE_LIMIT {
            exhausted = true;
            break;
        }
        if !safe_git_blob_path(path) || !path_filter_allows(path, &request.path_filters) {
            continue;
        }
        if request.exclude_generated && generated_detection::path_has_generated_signal(path) {
            continue;
        }
        let language = language_id(path).unwrap_or("unknown");
        if !language_filter_allows(path, language, &request.language_filters) {
            continue;
        }
        if seen.insert(path.clone()) {
            paths.push(path.clone());
        }
    }
    CandidatePaths {
        paths,
        degraded_reason: exhausted
            .then(|| "source fallback candidate file budget exhausted".to_owned()),
    }
}

fn path_filter_allows(path: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            let filter = normalize_filter_path(filter);
            filter == "." || path == filter || path.starts_with(&format!("{filter}/"))
        })
}

fn language_filter_allows(path: &str, language_id: &str, filters: &[String]) -> bool {
    filters.is_empty()
        || filters.iter().any(|filter| {
            filter == language_id
                || c_cpp_header_only_filter_allows(path, language_id, filter)
                || cxx_header_filter_allows(path, language_id, filter)
                || unknown_filter_allows_document_path(path, language_id, filter)
        })
}

fn c_cpp_header_only_filter_allows(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "__relay_c_cpp_header_only__"
        && matches!(language_id, "c" | "cpp")
        && c_cpp_header_path(path)
}

fn cxx_header_filter_allows(path: &str, language_id: &str, filter: &str) -> bool {
    filter == "cpp" && language_id == "c" && path.to_ascii_lowercase().ends_with(".h")
}

fn c_cpp_header_path(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
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

fn normalize_filter_path(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

#[cfg(test)]
#[path = "candidate_scope_tests.rs"]
mod tests;
