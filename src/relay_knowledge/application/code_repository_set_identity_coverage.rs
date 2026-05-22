use std::collections::{BTreeMap, BTreeSet};

use super::code_repository_set_plan::api_query_identity_leaves;
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
mod tests {
    use super::*;
    use crate::domain::{
        CodeRepositorySetMember, CodeRepositorySetMemberStatus, CodeRetrievalHit,
        RepositoryCodeRange,
    };

    #[test]
    fn identity_coverage_selects_distinct_dependency_api_symbols() {
        let app = member_status("samples", "scope-samples", 10);
        let sdk = member_status("sdk", "scope-sdk", 0);
        let mut selected = BTreeSet::from([0, 1, 5]);
        let results = vec![
            result(
                &app,
                1,
                26.0,
                "samples/one.go",
                "worker.New RegisterWorkflow",
            ),
            result(
                &app,
                2,
                25.0,
                "samples/two.go",
                "worker.New RegisterActivity",
            ),
            result(&app, 3, 24.0, "samples/three.go", "worker.InterruptCh"),
            result(&app, 4, 23.0, "samples/four.go", "RegisterWorkflow"),
            result(&app, 5, 22.0, "samples/five.go", "RegisterActivity"),
            symbol_result(
                &sdk,
                10,
                16.8,
                "worker/worker.go",
                "repo://sdk/worker::worker::InterruptCh",
                "func InterruptCh() <-chan interface{}",
            ),
            symbol_result(
                &sdk,
                11,
                10.5,
                "worker/worker.go",
                "repo://sdk/worker::worker::New",
                "func New(client Client, taskQueue string) Worker",
            ),
            symbol_result(
                &sdk,
                12,
                9.0,
                "internal/noise.go",
                "repo://sdk/internal::Other",
                "func Other()",
            ),
        ];

        select_identity_coverage_results(
            &results,
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            5,
            &mut selected,
        );

        assert!(selected.contains(&6));
        assert!(!selected.contains(&7));
    }

    #[test]
    fn identity_coverage_ignores_lexical_mentions() {
        let app = member_status("samples", "scope-samples", 10);
        let mut selected = BTreeSet::new();
        let results = vec![result(
            &app,
            1,
            30.0,
            "samples/main.go",
            "worker.New RegisterWorkflow RegisterActivity",
        )];

        select_identity_coverage_results(
            &results,
            "worker.New RegisterWorkflow RegisterActivity",
            3,
            &mut selected,
        );

        assert!(selected.is_empty());
    }

    fn member_status(
        repository_alias: &str,
        source_scope: &str,
        priority: i32,
    ) -> CodeRepositorySetMemberStatus {
        CodeRepositorySetMemberStatus {
            member: CodeRepositorySetMember {
                set_id: "set-workspace".to_owned(),
                repository_id: format!("repo-{repository_alias}"),
                repository_alias: repository_alias.to_owned(),
                ref_selector: "HEAD".to_owned(),
                resolved_commit_sha: format!("commit-{source_scope}"),
                source_scope: source_scope.to_owned(),
                path_filters: Vec::new(),
                language_filters: vec!["go".to_owned()],
                priority,
            },
            tree_hash: format!("tree-{source_scope}"),
            freshness_state: "fresh".to_owned(),
            stale: false,
            indexed_file_count: 1,
            symbol_count: 1,
            reference_count: 0,
            chunk_count: 1,
            degraded_reason: None,
        }
    }

    fn result(
        member: &CodeRepositorySetMemberStatus,
        line: u32,
        score: f64,
        path: &str,
        excerpt: &str,
    ) -> CodeRepositorySetQueryHit {
        let mut result = symbol_result(member, line, score, path, "", excerpt);
        result.hit.retrieval_layers = vec![CodeRetrievalLayer::Lexical];
        result.hit.canonical_symbol_id = None;
        result
    }

    fn symbol_result(
        member: &CodeRepositorySetMemberStatus,
        line: u32,
        score: f64,
        path: &str,
        canonical_symbol_id: &str,
        excerpt: &str,
    ) -> CodeRepositorySetQueryHit {
        CodeRepositorySetQueryHit {
            member: member.member.clone(),
            hit: CodeRetrievalHit {
                repository_id: member.member.repository_id.clone(),
                scope_id: member.member.source_scope.clone(),
                resolved_commit_sha: member.member.resolved_commit_sha.clone(),
                tree_hash: "tree".to_owned(),
                path: path.to_owned(),
                language_id: "go".to_owned(),
                byte_range: RepositoryCodeRange { start: 0, end: 10 },
                line_range: RepositoryCodeRange {
                    start: line,
                    end: line,
                },
                symbol_snapshot_id: Some(format!("symbol-{line}")),
                canonical_symbol_id: Some(canonical_symbol_id.to_owned()),
                file_id: Some(format!("file-{line}")),
                retrieval_layers: vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition],
                index_versions: vec!["code:1".to_owned()],
                stale: false,
                degraded_reason: None,
                edge_kind: None,
                edge_resolution_state: None,
                edge_target_hint: None,
                edge_confidence_basis_points: None,
                edge_confidence_tier: None,
                score,
                excerpt: excerpt.to_owned(),
            },
            overlay_evidence: Vec::new(),
            score,
        }
    }
}
