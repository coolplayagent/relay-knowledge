use super::*;
use crate::domain::CodeRetrievalLayer;

const TEST_SCOPE: &str = "code:test:chunk-search";

#[test]
fn strict_and_broad_chunk_merge_keeps_union_bounded_and_deduped() {
    let mut strict_hit = chunk_hit("client.Dial MustLoadDefaultClientOptions");
    strict_hit.score = 12.0;
    let mut duplicate_broad_hit = chunk_hit("client.Dial MustLoadDefaultClientOptions");
    duplicate_broad_hit.score = 1.0;
    let mut broad_hit = chunk_hit("worker.New RegisterWorkflow");
    broad_hit.score = 10.0;
    let mut tail_hit = chunk_hit("RegisterActivity InterruptCh");
    tail_hit.score = 2.0;

    let merged = merge_strict_and_broad_chunk_hits(
        vec![strict_hit],
        vec![duplicate_broad_hit, broad_hit, tail_hit],
        2,
    );

    assert_eq!(merged.len(), 2);
    assert_eq!(
        merged
            .iter()
            .filter(|hit| hit.excerpt.contains("MustLoadDefaultClientOptions"))
            .count(),
        1
    );
    assert!(merged.iter().any(|hit| hit.score == 12.0));
    assert!(
        !merged
            .iter()
            .any(|hit| hit.excerpt == "RegisterActivity InterruptCh")
    );
}

fn chunk_hit(excerpt: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: TEST_SCOPE.to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "src/main.go".to_owned(),
        language_id: "go".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Lexical],
        index_versions: Vec::new(),
        stale: false,
        staleness_hint: None,
        score: 1.0,
        excerpt: excerpt.to_owned(),
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
    }
}
