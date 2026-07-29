use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy, RepositoryCodeRange};

#[test]
fn hybrid_direct_gate_accepts_collective_lexical_surface_coverage() {
    let request = language_request(
        "typed arrow payload projector trim provider record",
        "typescript",
        12,
    );
    let hits = vec![
        lexical_hit(
            "src/provider.ts",
            "typescript",
            8.0,
            "record(payload: string): string { return trimPayload(payload); }",
        ),
        lexical_hit(
            "src/protocol.ts",
            "typescript",
            7.0,
            "export type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;",
        ),
        lexical_hit(
            "src/provider.ts",
            "typescript",
            6.0,
            "export class ProviderRuntime { record(payload: string) { return payload; } }",
        ),
    ];

    assert!(hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_direct_gate_keeps_sparse_lexical_surface_for_graph_expansion() {
    let request = request(
        "typed arrow payload projector trim provider record",
        CodeQueryKind::Hybrid,
        12,
    );
    let hits = vec![
        lexical_hit("src/provider.ts", "typescript", 8.0, "provider payload"),
        lexical_hit("src/protocol.ts", "typescript", 7.0, "projector trim"),
    ];

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_direct_gate_keeps_declaration_intent_for_graph_expansion() {
    let request = request(
        "decorated async service overload exception subclass normalize payload",
        CodeQueryKind::Hybrid,
        12,
    );
    let hits = vec![
        lexical_hit(
            "syntax_service/service.py",
            "python",
            10.0,
            "raise OverloadedServiceError normalize payload async service",
        ),
        lexical_hit(
            "syntax_service/decorators.py",
            "python",
            6.0,
            "def traced_operation(name): async wrapper",
        ),
    ];

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_direct_gate_keeps_conversion_chunk_intent_for_graph_expansion() {
    let request = request(
        "openai responses tool calls function_call_output convert common chunk",
        CodeQueryKind::Hybrid,
        12,
    );
    let hits = vec![
        lexical_hit(
            "src/provider/openai.ts",
            "typescript",
            9.0,
            "function_call_output responses provider chunk convert",
        ),
        lexical_hit(
            "src/provider/common.ts",
            "typescript",
            6.0,
            "common chunk conversion maps tool calls",
        ),
    ];

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_direct_gate_keeps_inline_lambda_intent_for_graph_expansion() {
    let request = request(
        "kotlin lambda request handler timeout default trim",
        CodeQueryKind::Hybrid,
        12,
    );
    let hits = vec![
        lexical_hit(
            "src/main/kotlin/example/Client.kt",
            "kotlin",
            12.0,
            "fun defaultHandler(): RequestHandler = { value -> value.trim() }",
        ),
        lexical_hit(
            "src/main/kotlin/example/Client.kt",
            "kotlin",
            10.0,
            "fun withTimeout(timeout: Duration): SyntaxClient = SyntaxClient { value -> value }",
        ),
    ];

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_direct_gate_keeps_underfilled_long_lexical_surface_for_fallback_layers() {
    let request = language_request(
        "external session workflow TypeScript client openExternalSession",
        "typescript",
        12,
    );
    let hits = vec![
        lexical_hit(
            "external_deps/ts_sdk/sessionClient.ts",
            "typescript",
            16.0,
            "openExternalSession(payload: string) creates an external TypeScript session client workflow",
        ),
        lexical_hit(
            "external_deps/ts_sdk/sessionClient.ts",
            "typescript",
            13.0,
            "ExternalTypeScriptSessionClient openExternalSession(payload: string)",
        ),
    ];

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_direct_gate_accepts_collective_api_identity_symbol_coverage() {
    let request = request(
        "worker.New RegisterWorkflow InterruptCh task queue",
        CodeQueryKind::Hybrid,
        5,
    );
    let hits = vec![
        symbol_hit("worker.New", "fn New(client Client) Worker"),
        symbol_hit(
            "worker.RegisterWorkflow",
            "fn RegisterWorkflow(workflow interface{})",
        ),
        symbol_hit("worker.InterruptCh", "fn InterruptCh() <-chan interface{}"),
    ];

    assert!(hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_direct_gate_accepts_pascal_type_identifier_hits() {
    let request = request(
        "EvalCheckpointStore signature mismatch append result",
        CodeQueryKind::Hybrid,
        10,
    );
    let hits = vec![lexical_hit(
        "src/checkpoint.py",
        "python",
        10.0,
        "class EvalCheckpointStore: def append_result(self, result): ...",
    )];

    assert!(hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits
    ));
}

#[test]
fn hybrid_direct_gate_keeps_graph_expansion_for_api_graph_intent_or_underfilled_limits() {
    let hits = vec![
        symbol_hit("worker.New", "fn New(client Client) Worker"),
        symbol_hit(
            "worker.RegisterWorkflow",
            "fn RegisterWorkflow(workflow interface{})",
        ),
    ];

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request(
            "worker.New RegisterWorkflow callers",
            CodeQueryKind::Hybrid,
            5,
        ),
        &hits,
    ));
    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request("worker.New RegisterWorkflow", CodeQueryKind::Hybrid, 1),
        &hits,
    ));
}

#[test]
fn hybrid_direct_gate_requires_scoped_api_identity_symbol_match() {
    let request = request("worker.New RegisterWorkflow", CodeQueryKind::Hybrid, 5);
    let hits = vec![
        symbol_hit("internal.New", "fn New(client Client) Worker"),
        symbol_hit(
            "worker.RegisterWorkflow",
            "fn RegisterWorkflow(workflow interface{})",
        ),
    ];

    assert!(!hybrid_direct_results_can_answer_without_graph_expansion(
        &request, &hits,
    ));
}

fn request(query: &str, kind: CodeQueryKind, limit: usize) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        kind,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn language_request(query: &str, language: &str, limit: usize) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec![language.to_owned()])
            .expect("selector should validate"),
        CodeQueryKind::Hybrid,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn lexical_hit(path: &str, language_id: &str, score: f64, excerpt: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "code:test:hybrid-direct-gate:commit:tree".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        byte_range: range(1, 1),
        line_range: range(1, 1),
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Lexical],
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

fn symbol_hit(canonical_symbol_id: &str, excerpt: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "code:test:hybrid-direct-gate:commit:tree".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "src/worker.go".to_owned(),
        language_id: "go".to_owned(),
        byte_range: range(1, 1),
        line_range: range(1, 1),
        symbol_snapshot_id: Some(format!("{canonical_symbol_id}-symbol")),
        canonical_symbol_id: Some(format!("repo://repo/{canonical_symbol_id}")),
        file_id: Some("worker-file".to_owned()),
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
        score: 10.0,
        excerpt: excerpt.to_owned(),
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}
