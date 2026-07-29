use std::collections::{BTreeMap, BTreeSet};

use super::plan::api_query_identity_leaves;
use crate::domain::{CodeRepositorySetQueryHit, CodeRetrievalLayer};

const MIN_IDENTITY_COVERAGE_IDENTITIES: usize = 2;
const MAX_IDENTITY_COVERAGE_PER_MEMBER: usize = 3;
const IDENTITY_COVERAGE_MIN_RELATIVE_SCORE: f64 = 0.30;
const IDENTITY_COVERAGE_MAX_SCORE_GAP: f64 = 18.0;

pub(super) fn select_identity_coverage_results(
    results: &[CodeRepositorySetQueryHit],
    query: &str,
    limit: usize,
    selected: &mut BTreeSet<usize>,
) {
    if selected.len() >= limit || results.is_empty() {
        return;
    }
    let identities = api_query_identity_leaves(query);
    if identities.len() < MIN_IDENTITY_COVERAGE_IDENTITIES {
        return;
    }
    let score_floor = identity_coverage_score_floor(results[0].score);
    let member_order = repository_set_member_order(results);
    let mut covered = covered_member_identities(results, &identities, selected);
    let mut added_by_member = BTreeMap::<(String, String), usize>::new();

    for member_key in member_order {
        for identity in &identities {
            if selected.len() >= limit {
                return;
            }
            if added_by_member.get(&member_key).copied().unwrap_or(0)
                >= MAX_IDENTITY_COVERAGE_PER_MEMBER
            {
                break;
            }
            if covered.contains(&(member_key.clone(), identity.clone())) {
                continue;
            }
            let Some(index) = results.iter().enumerate().position(|(index, result)| {
                !selected.contains(&index)
                    && result.score >= score_floor
                    && repository_set_member_key(result) == member_key
                    && result_matches_identity(result, identity)
            }) else {
                continue;
            };
            selected.insert(index);
            covered.insert((member_key.clone(), identity.clone()));
            *added_by_member.entry(member_key.clone()).or_insert(0) += 1;
        }
    }
}

fn covered_member_identities(
    results: &[CodeRepositorySetQueryHit],
    identities: &[String],
    selected: &BTreeSet<usize>,
) -> BTreeSet<((String, String), String)> {
    let mut covered = BTreeSet::new();
    for index in selected {
        let Some(result) = results.get(*index) else {
            continue;
        };
        let member_key = repository_set_member_key(result);
        for identity in identities {
            if result_matches_identity(result, identity) {
                covered.insert((member_key.clone(), identity.clone()));
            }
        }
    }

    covered
}

fn result_matches_identity(result: &CodeRepositorySetQueryHit, identity: &str) -> bool {
    hit_has_symbol_surface(result)
        && (result
            .hit
            .canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol_id| canonical_symbol_leaf_matches(symbol_id, identity))
            || text_contains_identifier(&result.hit.excerpt, identity))
}

fn hit_has_symbol_surface(result: &CodeRepositorySetQueryHit) -> bool {
    result.hit.retrieval_layers.iter().any(|layer| {
        matches!(
            layer,
            CodeRetrievalLayer::Symbol | CodeRetrievalLayer::Definition
        )
    })
}

fn identity_coverage_score_floor(best_score: f64) -> f64 {
    if best_score <= 0.0 {
        return f64::INFINITY;
    }

    (best_score * IDENTITY_COVERAGE_MIN_RELATIVE_SCORE)
        .max(best_score - IDENTITY_COVERAGE_MAX_SCORE_GAP)
}

fn repository_set_member_order(results: &[CodeRepositorySetQueryHit]) -> Vec<(String, String)> {
    let mut members = Vec::new();
    for result in results {
        let key = repository_set_member_key(result);
        if !members.contains(&key) {
            members.push(key);
        }
    }

    members
}

fn repository_set_member_key(result: &CodeRepositorySetQueryHit) -> (String, String) {
    (
        result.member.repository_id.clone(),
        result.member.source_scope.clone(),
    )
}

fn canonical_symbol_leaf_matches(canonical_symbol_id: &str, identity: &str) -> bool {
    canonical_symbol_id
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|term| !term.is_empty())
        .is_some_and(|leaf| leaf == identity)
}

fn text_contains_identifier(text: &str, identity: &str) -> bool {
    text.match_indices(identity).any(|(start, _)| {
        let end = start + identity.len();
        text.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|character| !is_identifier_char(character))
        }) && text.get(end..).is_some_and(|suffix| {
            suffix
                .chars()
                .next()
                .is_none_or(|character| !is_identifier_char(character))
        })
    })
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

#[cfg(test)]
#[path = "identity_coverage_tests.rs"]
mod tests;
