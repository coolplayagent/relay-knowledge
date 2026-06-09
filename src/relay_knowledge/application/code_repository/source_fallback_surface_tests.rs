use super::*;
use crate::{
    code::{SourceGrepKind, SourceGrepMatch, SourceGrepOutcome},
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange, code_snapshot_scope_id,
    },
};

#[test]
fn hybrid_source_surface_refreshes_match_inside_structured_line_range() {
    let request = request(
        "external session workflow TypeScript client openExternalSession",
        CodeQueryKind::Hybrid,
    );
    let plan = CodeGrepFallbackPlan {
        commit: "commit".to_owned(),
        query: "ExternalTypeScriptSessionClient".to_owned(),
        paths: vec!["src/application.ts".to_owned()],
        path_filters: Vec::new(),
        language_filters: vec!["typescript".to_owned()],
        limit: 12,
        kind: SourceGrepKind::Hybrid,
        identity: None,
        exclude_generated: false,
        needs_scope_paths: false,
    };
    let mut workflow_result = hit(
        "src/application.ts",
        "export function runExternalSessionWorkflow(payload: string): string {",
    );
    workflow_result.language_id = "typescript".to_owned();
    workflow_result.line_range = RepositoryCodeRange { start: 4, end: 7 };
    workflow_result.retrieval_layers =
        vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    workflow_result.canonical_symbol_id = Some("repo://repo/src::application::client".to_owned());
    let mut results = vec![workflow_result];

    append_code_grep_fallback(
        &status(),
        &request,
        &mut results,
        &plan,
        SourceGrepOutcome {
            matches: vec![SourceGrepMatch {
                path: "src/application.ts".to_owned(),
                language_id: "typescript".to_owned(),
                excerpt: "const client = new ExternalTypeScriptSessionClient();".to_owned(),
                byte_range: RepositoryCodeRange {
                    start: 120,
                    end: 152,
                },
                line_range: RepositoryCodeRange { start: 5, end: 5 },
                is_generated: false,
            }],
            degraded_reason: None,
        },
    );

    assert_eq!(results.len(), 1);
    assert!(
        results[0]
            .excerpt
            .contains("new ExternalTypeScriptSessionClient")
    );
    assert_eq!(
        results[0].line_range,
        RepositoryCodeRange { start: 4, end: 7 }
    );
    assert!(
        results[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[test]
fn hybrid_source_surface_fallback_skips_complete_exported_value_surfaces() {
    let request = request(
        "typed arrow payload projector trim provider record",
        CodeQueryKind::Hybrid,
    );
    let mut result = hit(
        "src/protocol.ts",
        "export const trimPayload: PayloadProjector<string> = (payload) => payload.trim();",
    );
    result.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    result.canonical_symbol_id = Some("repo://repo/src::protocol::trimPayload".to_owned());
    let mut type_result = hit(
        "src/protocol.ts",
        "export type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;",
    );
    type_result.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    type_result.canonical_symbol_id =
        Some("repo://repo/src::protocol::PayloadProjector".to_owned());
    let mut contextual_type_result = hit("src/provider.ts", "PayloadProjector<string>");
    contextual_type_result.retrieval_layers =
        vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    contextual_type_result.canonical_symbol_id =
        Some("repo://repo/src::protocol::PayloadProjector".to_owned());

    assert!(
        plan_code_grep_fallback(
            &status(),
            &request,
            &[contextual_type_result, result, type_result]
        )
        .is_none()
    );
}

fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector =
        crate::domain::CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
    CodeRetrievalRequest::new(
        query,
        selector,
        kind,
        10,
        crate::domain::FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
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

fn hit(path: &str, excerpt: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: Some("symbol".to_owned()),
        canonical_symbol_id: Some("repo://repo/src::context".to_owned()),
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Lexical],
        index_versions: vec!["code:scope:tree".to_owned()],
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 2.0,
        excerpt: excerpt.to_owned(),
    }
}
