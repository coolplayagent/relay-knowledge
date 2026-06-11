use super::*;
use crate::domain::{RepositoryCodeRange, code_snapshot_scope_id};

#[test]
fn source_fallback_append_preserves_matching_name_filters() {
    let request = request("name:rk_read_fn rk_read_fn");
    let plan = definition_plan("rk_read_fn");
    let mut results = Vec::new();

    append_code_grep_fallback(
        &status(),
        &request,
        &mut results,
        &plan,
        fallback_outcome("typedef int (*rk_read_fn)(struct rk_device *dev);"),
    );

    assert_eq!(results.len(), 1);
    assert!(results[0].excerpt.contains("rk_read_fn"));
}

#[test]
fn source_fallback_append_rejects_unmatched_name_filters() {
    let request = request("name:other rk_read_fn");
    let plan = definition_plan("rk_read_fn");
    let mut results = Vec::new();

    append_code_grep_fallback(
        &status(),
        &request,
        &mut results,
        &plan,
        fallback_outcome("typedef int (*rk_read_fn)(struct rk_device *dev);"),
    );

    assert!(results.is_empty());
}

fn request(query: &str) -> CodeRetrievalRequest {
    let selector = crate::domain::CodeRepositorySelector::new(
        "repo",
        "commit",
        Vec::new(),
        vec!["c".to_owned()],
    )
    .expect("selector should validate");
    CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::Definition,
        10,
        crate::domain::FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn definition_plan(query: &str) -> CodeGrepFallbackPlan {
    CodeGrepFallbackPlan {
        commit: "commit".to_owned(),
        query: query.to_owned(),
        paths: Vec::new(),
        path_filters: Vec::new(),
        language_filters: vec!["c".to_owned()],
        limit: 10,
        kind: SourceGrepKind::Definition,
        identity: Some(query.to_owned()),
        exclude_generated: false,
        needs_scope_paths: false,
    }
}

fn fallback_outcome(excerpt: &str) -> SourceGrepOutcome {
    SourceGrepOutcome {
        matches: vec![SourceGrepMatch {
            path: "include/driver_ops.h".to_owned(),
            language_id: "c".to_owned(),
            excerpt: excerpt.to_owned(),
            byte_range: RepositoryCodeRange {
                start: 0,
                end: excerpt.len() as u32,
            },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
            is_generated: false,
        }],
        degraded_reason: None,
    }
}

fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some(code_snapshot_scope_id("repo", "tree", &[], &[])),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        stale: false,
        degraded_reason: None,
    }
}
