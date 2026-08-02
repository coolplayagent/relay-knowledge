use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy, RepositoryCodeRange};

#[test]
fn callees_project_the_resolved_callee_identity() {
    let status = CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 1,
        symbol_count: 2,
        reference_count: 0,
        chunk_count: 1,
        stale: false,
        degraded_reason: None,
    };
    let request = CodeRetrievalRequest::new(
        "run",
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let rows = vec![CallRow {
        file_id: "file".to_owned(),
        path: "src/service.rs".to_owned(),
        language_id: "rust".to_owned(),
        caller_symbol_snapshot_id: Some("caller".to_owned()),
        caller_name: Some("run".to_owned()),
        callee_symbol_snapshot_id: Some("callee".to_owned()),
        callee_name: "dispatch".to_owned(),
        line_range: RepositoryCodeRange { start: 5, end: 5 },
        caller_line_range: Some(RepositoryCodeRange { start: 1, end: 10 }),
        target_hint: Some("Service.dispatch".to_owned()),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 10_000,
        confidence_tier: "exact".to_owned(),
        caller_canonical_symbol_id: Some("Service.run".to_owned()),
        callee_canonical_symbol_id: Some("Service.dispatch".to_owned()),
        caller_signature: Some("fn run()".to_owned()),
        callee_signature: Some("fn dispatch()".to_owned()),
        caller_excerpt: Some("fn run() { dispatch(); }".to_owned()),
        callee_excerpt: Some("fn dispatch() {}".to_owned()),
        is_generated: false,
    }];

    let hits = call_rows_to_hits(&status, &request, rows);

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].symbol_snapshot_id.as_deref(), Some("callee"));
    assert_eq!(
        hits[0].canonical_symbol_id.as_deref(),
        Some("Service.dispatch")
    );
}
