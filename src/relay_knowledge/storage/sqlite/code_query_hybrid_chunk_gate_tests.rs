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
fn hybrid_chunk_gate_accepts_collective_dense_coverage_before_limit_is_full() {
    let request = hybrid_gate_request(
        "tsx provider panel effect run provider envelope payload",
        12,
    );
    let hits = vec![
        chunk_gate_hit(
            "function ProviderPanel() {\nReact.useEffect(() => runProvider(envelope.payload));\n}",
        ),
        chunk_gate_hit("export { ProviderPanel } from './component';"),
        chunk_gate_hit("return sendEnvelope(runtime, payload) from provider flow"),
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

    let wide_request = hybrid_gate_request(
        "tsx provider panel effect run provider envelope payload",
        12,
    );
    assert!(!hybrid_chunk_results_can_answer_without_graph_expansion(
        &wide_request,
        &[
            chunk_gate_hit("ProviderPanel renders provider payload"),
            chunk_gate_hit("provider payload envelope"),
            chunk_gate_hit("payload provider envelope"),
        ]
    ));
}

#[test]
fn strict_hybrid_chunk_fts_uses_multiple_structured_api_anchors() {
    let strict = strict_hybrid_chunk_fts_match_query(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
    )
    .expect("multiple API anchors should enable strict chunk recall");

    assert_eq!(
        strict,
        "\"RegisterWorkflow\" \"RegisterActivity\" \"InterruptCh\""
    );
    assert!(strict_hybrid_chunk_fts_match_query("RK_PIPELINE_NOTE").is_none());
    assert!(
        strict_hybrid_chunk_fts_match_query(
            "client.Dial envconfig MustLoadDefaultClientOptions workflow client"
        )
        .is_none()
    );
}

#[test]
fn strict_hybrid_chunk_candidate_limit_stays_bounded() {
    assert_eq!(
        strict_hybrid_chunk_candidate_limit(&hybrid_gate_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            10,
        )),
        120
    );
    assert_eq!(
        strict_hybrid_chunk_candidate_limit(&hybrid_gate_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            40,
        )),
        180
    );
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
