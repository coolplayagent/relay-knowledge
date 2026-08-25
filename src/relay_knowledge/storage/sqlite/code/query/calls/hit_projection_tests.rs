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

#[test]
fn callers_rank_production_symbols_before_embedded_test_symbols() {
    let status = status();
    let request = request("check_claims_from_token", CodeQueryKind::Callers);
    let rows = vec![
        caller_row(
            "test_check_claims_from_token_expired_credentials",
            "repo://repo/src::auth::tests.test_check_claims_from_token_expired_credentials",
            100,
        ),
        caller_row(
            "check_key_valid",
            "repo://repo/src::auth::check_key_valid",
            20,
        ),
    ];

    let mut hits = call_rows_to_hits(&status, &request, rows);
    hits.sort_by(|left, right| right.score.total_cmp(&left.score));

    assert!(hits[0].excerpt.starts_with("check_key_valid calls"));
    assert!(hits[0].score > hits[1].score);
}

#[test]
fn explicit_test_caller_queries_disable_symbol_context_penalty() {
    let request = request("test check_claims_from_token", CodeQueryKind::Callers);

    assert_eq!(
        caller_test_context_penalty(
            4.0,
            Some("test_check_claims_from_token"),
            Some("repo://repo/src::auth::tests.test_check_claims_from_token"),
            &request,
            true,
        ),
        0.0
    );
}

#[test]
fn caller_test_context_demotion_preserves_positive_evidence() {
    let request = request("dispatch", CodeQueryKind::Callers);
    for identity in ["TestDispatcher", "testDispatcher"] {
        let score = 0.2 + caller_test_context_penalty(0.2, Some(identity), None, &request, false);
        assert!(score > 0.0, "{identity}");
        assert!(score < 0.2, "{identity}");
    }
}

fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
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
    }
}

fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        kind,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn caller_row(caller_name: &str, canonical_id: &str, line: u32) -> CallRow {
    CallRow {
        file_id: "file".to_owned(),
        path: "src/auth.rs".to_owned(),
        language_id: "rust".to_owned(),
        caller_symbol_snapshot_id: Some(format!("caller-{line}")),
        caller_name: Some(caller_name.to_owned()),
        callee_symbol_snapshot_id: Some("callee".to_owned()),
        callee_name: "check_claims_from_token".to_owned(),
        line_range: RepositoryCodeRange {
            start: line,
            end: line,
        },
        caller_line_range: Some(RepositoryCodeRange {
            start: line,
            end: line + 5,
        }),
        target_hint: Some("check_claims_from_token".to_owned()),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 8_000,
        confidence_tier: "inferred".to_owned(),
        caller_canonical_symbol_id: Some(canonical_id.to_owned()),
        callee_canonical_symbol_id: Some(
            "repo://repo/src::auth::check_claims_from_token".to_owned(),
        ),
        caller_signature: Some(format!("fn {caller_name}()")),
        callee_signature: Some("fn check_claims_from_token()".to_owned()),
        caller_excerpt: Some(format!(
            "fn {caller_name}() {{ check_claims_from_token(); }}"
        )),
        callee_excerpt: Some("fn check_claims_from_token() {}".to_owned()),
        is_generated: false,
    }
}
