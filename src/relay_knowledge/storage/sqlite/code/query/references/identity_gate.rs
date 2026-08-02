use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::super::relevance::{CandidateLayer, SymbolIdentityQuery, candidate_limit};

pub(super) fn reference_identity_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
    hit_count: usize,
    saturated: bool,
) -> bool {
    hit_count > 0
        && !saturated
        && request.code_query_kind == CodeQueryKind::References
        && (identity.is_scoped()
            || (hit_count <= request.limit
                && specific_reference_identity_leaf(identity.leaf_name())))
}

pub(super) fn reference_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Reference).min(200)
}

fn specific_reference_identity_leaf(leaf_name: &str) -> bool {
    leaf_name.len() >= 8 || leaf_name.contains('_') || has_case_boundary(leaf_name)
}

fn has_case_boundary(value: &str) -> bool {
    let mut previous: Option<char> = None;
    for character in value.chars() {
        if character.is_ascii_uppercase()
            && previous.is_some_and(|previous| previous.is_ascii_lowercase())
        {
            return true;
        }
        previous = Some(character);
    }

    false
}

#[cfg(test)]
#[path = "identity_gate_tests.rs"]
mod tests;
