use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn name_filter_matches_symbol_identity_not_excerpt_body() {
    let mut hits = vec![
        test_hit(
            "src/search.rs",
            Some("repo::search_code"),
            Some("sym-search"),
            vec![CodeRetrievalLayer::Symbol],
            "pub fn search_code() { api_client(); }",
            5.0,
        ),
        test_hit(
            "src/api.rs",
            Some("repo::search_code_api"),
            Some("sym-api"),
            vec![CodeRetrievalLayer::Symbol],
            "pub fn search_code_api() {}",
            4.0,
        ),
    ];
    let request = request("name:api search_code");

    filter_dedupe_sort_truncate(&mut hits, &request);

    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].canonical_symbol_id.as_deref(),
        Some("repo::search_code_api")
    );
}

#[test]
fn kind_filter_removes_non_symbol_layer_hits() {
    let mut hits = vec![
        test_hit(
            "src/chunk.rs",
            None,
            None,
            vec![CodeRetrievalLayer::Lexical],
            "retry_policy function docs",
            9.0,
        ),
        test_hit(
            "src/symbol.rs",
            Some("repo::retry_policy"),
            Some("sym-retry"),
            vec![CodeRetrievalLayer::Symbol],
            "pub fn retry_policy() {}",
            4.0,
        ),
    ];
    let request = request("kind:function retry_policy");

    filter_dedupe_sort_truncate(&mut hits, &request);

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/symbol.rs");
}

#[test]
fn name_filter_matches_sbom_dependency_identity() {
    let mut dependency_hit = test_hit(
        "Cargo.toml",
        None,
        None,
        vec![CodeRetrievalLayer::Sbom],
        "cargo serde group=dependencies",
        5.0,
    );
    dependency_hit.edge_kind = Some("dependency".to_owned());
    dependency_hit.edge_target_hint = Some("serde".to_owned());
    let mut unrelated_dependency = test_hit(
        "package.json",
        None,
        None,
        vec![CodeRetrievalLayer::Sbom],
        "npm react group=dependencies",
        4.0,
    );
    unrelated_dependency.edge_kind = Some("dependency".to_owned());
    unrelated_dependency.edge_target_hint = Some("react".to_owned());
    let mut hits = vec![dependency_hit, unrelated_dependency];
    let request = request("name:serde serde");

    filter_dedupe_sort_truncate(&mut hits, &request);

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].edge_target_hint.as_deref(), Some("serde"));
}

fn request(query: &str) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
            .expect("selector validates"),
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request validates")
}

fn test_hit(
    path: &str,
    canonical_symbol_id: Option<&str>,
    symbol_snapshot_id: Option<&str>,
    retrieval_layers: Vec<CodeRetrievalLayer>,
    excerpt: &str,
    score: f64,
) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 0 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: symbol_snapshot_id.map(ToOwned::to_owned),
        canonical_symbol_id: canonical_symbol_id.map(ToOwned::to_owned),
        file_id: Some(format!("{path}:file")),
        retrieval_layers,
        index_versions: Vec::new(),
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score,
        excerpt: excerpt.to_owned(),
    }
}
