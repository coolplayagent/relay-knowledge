use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

const TEST_SCOPE: &str = "code:test:hybrid-chunk-gate";

#[test]
fn hybrid_chunk_gate_accepts_dense_multi_identity_chunks() {
    let request = hybrid_gate_request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        3,
    );
    let hits = vec![
        chunk_gate_hit(
            "w := worker.New(c, taskQueue, worker.Options{})\nw.RegisterWorkflow(flow)\nw.RegisterActivity(activity)",
        ),
        chunk_gate_hit("err = w.Run(worker.InterruptCh()) with task queue shutdown"),
        chunk_gate_hit("RegisterWorkflow and RegisterActivity bind the worker task queue"),
    ];

    assert!(hybrid_chunk_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_chunk_gate_keeps_graph_expansion_for_sparse_or_fallback_hits() {
    let request = hybrid_gate_request("RK_PIPELINE_NOTE", 1);
    assert!(!hybrid_chunk_results_can_answer_without_graph_expansion(
        &request,
        &[chunk_gate_hit("RK_PIPELINE_NOTE records dispatch ordering")]
    ));

    let sequence_request = hybrid_gate_request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        2,
    );
    let fallback_hit = CodeRetrievalHit {
        retrieval_layers: vec![
            CodeRetrievalLayer::Lexical,
            CodeRetrievalLayer::TextFallback,
        ],
        ..chunk_gate_hit("worker.New RegisterWorkflow RegisterActivity InterruptCh")
    };
    assert!(!hybrid_chunk_results_can_answer_without_graph_expansion(
        &sequence_request,
        &[fallback_hit, chunk_gate_hit("worker.New RegisterWorkflow")]
    ));
}

fn hybrid_gate_request(query: &str, limit: usize) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should be valid");

    CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::Hybrid,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should be valid")
}

fn chunk_gate_hit(excerpt: &str) -> CodeRetrievalHit {
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
