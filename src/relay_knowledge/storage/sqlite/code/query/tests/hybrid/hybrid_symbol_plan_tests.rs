use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

const TEST_SOURCE_SCOPE: &str = "code:test:hybrid-symbol-plan:commit:tree";

#[test]
fn hybrid_symbol_plan_requires_unambiguous_symbol_window() {
    let read_request = request("read", CodeQueryKind::Hybrid, 2);
    let hits = vec![
        symbol_hit("one", "repo://repo/src::one::read", "fn read()"),
        symbol_hit("two", "repo://repo/src::two::read", "fn read()"),
        symbol_hit("three", "repo://repo/src::three::read", "fn read()"),
    ];

    assert!(!hybrid_symbol_query_can_answer_without_non_symbol_layers(
        &read_request,
        &hits
    ));
    assert!(!hybrid_symbol_query_can_answer_without_non_symbol_layers(
        &request("read flow", CodeQueryKind::Hybrid, 10),
        &hits[..1],
    ));
    assert!(hybrid_symbol_query_can_answer_without_non_symbol_layers(
        &request("DBImpl::Get", CodeQueryKind::Hybrid, 10),
        &[symbol_hit(
            "get",
            "repo://repo/db::DBImpl.Get",
            "Status DBImpl::Get(const ReadOptions& options)",
        )],
    ));
}

fn request(query: &str, kind: CodeQueryKind, limit: usize) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should be valid");
    CodeRetrievalRequest::new(query, selector, kind, limit, FreshnessPolicy::AllowStale)
        .expect("request should be valid")
}

fn symbol_hit(id: &str, canonical_symbol_id: &str, excerpt: &str) -> CodeRetrievalHit {
    let range = RepositoryCodeRange { start: 1, end: 1 };
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: TEST_SOURCE_SCOPE.to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: format!("src/{id}.rs"),
        language_id: "rust".to_owned(),
        byte_range: range.clone(),
        line_range: range,
        symbol_snapshot_id: Some(format!("{id}-symbol")),
        canonical_symbol_id: Some(canonical_symbol_id.to_owned()),
        file_id: Some(format!("{id}-file")),
        retrieval_layers: vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition],
        index_versions: Vec::new(),
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 8.0,
        excerpt: excerpt.to_owned(),
    }
}
