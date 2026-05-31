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
fn hybrid_direct_gate_accepts_dense_non_fallback_symbol_evidence() {
    let request = hybrid_gate_request("Recover descriptor save_manifest VersionEdit", 10);

    assert!(hybrid_direct_results_can_answer_without_graph_expansion(
        &request,
        &[symbol_gate_hit(
            "// Recover the descriptor from persistent storage.\nStatus Recover(VersionEdit* edit, bool* save_manifest);"
        )]
    ));

    let fallback_hit = CodeRetrievalHit {
        retrieval_layers: vec![
            CodeRetrievalLayer::Lexical,
            CodeRetrievalLayer::TextFallback,
        ],
        ..chunk_gate_hit("Recover descriptor VersionEdit save_manifest")
    };
    let call_graph_hit = CodeRetrievalHit {
        retrieval_layers: vec![CodeRetrievalLayer::CallGraph],
        edge_kind: Some("call".to_owned()),
        ..chunk_gate_hit("Recover descriptor VersionEdit save_manifest")
    };

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request,
        &[fallback_hit]
    ));
    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request,
        &[call_graph_hit]
    ));
}

#[test]
fn hybrid_direct_gate_keeps_graph_expansion_for_graph_intent_terms() {
    let request = hybrid_gate_request("Recover descriptor save_manifest VersionEdit callers", 10);

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request,
        &[symbol_gate_hit(
            "// Recover the descriptor from persistent storage.\nStatus Recover(VersionEdit* edit, bool* save_manifest);"
        )]
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
    let member_access_strict = strict_hybrid_chunk_fts_match_query(
        "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
    )
    .expect("member-access API leaves should complete a strict recall pair");
    assert_eq!(
        member_access_strict,
        "\"MustLoadDefaultClientOptions\" \"Dial\""
    );
    let sparse_member_access_strict = strict_hybrid_chunk_fts_match_query(
        "client.Dial MustLoadDefaultClientOptions setup call target api",
    )
    .expect("member-access leaves should allow strict recall with one structured API anchor");
    assert_eq!(
        sparse_member_access_strict,
        "\"MustLoadDefaultClientOptions\" \"Dial\""
    );
    assert!(
        strict_hybrid_chunk_fts_match_query("client.Dial workflow client path/to/client.go")
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
        60
    );
    assert_eq!(
        strict_hybrid_chunk_candidate_limit(&hybrid_gate_request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            40,
        )),
        120
    );
}

#[test]
fn strict_and_broad_chunk_merge_keeps_union_bounded_and_deduped() {
    let mut strict_hit = chunk_gate_hit("client.Dial MustLoadDefaultClientOptions");
    strict_hit.score = 12.0;
    let mut duplicate_broad_hit = chunk_gate_hit("client.Dial MustLoadDefaultClientOptions");
    duplicate_broad_hit.score = 1.0;
    let mut broad_hit = chunk_gate_hit("worker.New RegisterWorkflow");
    broad_hit.score = 10.0;
    let mut tail_hit = chunk_gate_hit("RegisterActivity InterruptCh");
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

fn symbol_gate_hit(excerpt: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        retrieval_layers: vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition],
        symbol_snapshot_id: Some("symbol".to_owned()),
        canonical_symbol_id: Some("repo://repo/src::DBImpl::Recover".to_owned()),
        ..chunk_gate_hit(excerpt)
    }
}
