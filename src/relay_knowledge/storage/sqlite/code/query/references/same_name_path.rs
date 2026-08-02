use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::identifier_text::normalized_identifier;

pub(super) fn reference_same_name_file_penalty(
    base_score: f64,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::References {
        return 0.0;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let file_stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    if normalized_identifier(file_stem) == normalized_identifier(&request.query) {
        -0.45
    } else {
        0.0
    }
}

#[cfg(test)]
#[path = "same_name_path_tests.rs"]
mod tests;
