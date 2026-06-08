use super::*;
use crate::{
    code::{SourceDeclarationMatch, SourceGrepKind, SourceGrepMatch, SourceGrepOutcome},
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange, code_snapshot_scope_id,
    },
};

#[test]
fn source_grep_fallback_demotes_generated_matches_without_excluding_them() {
    let request = request("RK_GENERATED_NOTE", CodeQueryKind::References);
    let plan = CodeGrepFallbackPlan {
        commit: "commit".to_owned(),
        query: "RK_GENERATED_NOTE".to_owned(),
        paths: Vec::new(),
        path_filters: Vec::new(),
        language_filters: vec!["typescript".to_owned()],
        limit: 10,
        kind: SourceGrepKind::References,
        identity: None,
        exclude_generated: false,
        needs_scope_paths: false,
    };
    let mut results = Vec::new();

    append_code_grep_fallback(
        &status(),
        &request,
        &mut results,
        &plan,
        SourceGrepOutcome {
            matches: vec![
                SourceGrepMatch {
                    path: "dist/client.ts".to_owned(),
                    language_id: "typescript".to_owned(),
                    excerpt: "export const RK_GENERATED_NOTE = true;".to_owned(),
                    byte_range: RepositoryCodeRange { start: 0, end: 38 },
                    line_range: RepositoryCodeRange { start: 1, end: 1 },
                    is_generated: true,
                },
                SourceGrepMatch {
                    path: "src/client.ts".to_owned(),
                    language_id: "typescript".to_owned(),
                    excerpt: "export const RK_GENERATED_NOTE = true;".to_owned(),
                    byte_range: RepositoryCodeRange { start: 0, end: 38 },
                    line_range: RepositoryCodeRange { start: 1, end: 1 },
                    is_generated: false,
                },
            ],
            degraded_reason: None,
        },
    );

    let generated_score = score_for_path(&results, "dist/client.ts");
    let handwritten_score = score_for_path(&results, "src/client.ts");

    assert!(generated_score > 0.0);
    assert!(generated_score < handwritten_score);
}

#[test]
fn declaration_source_fallback_demotes_generated_matches_without_excluding_them() {
    let request = request("RK_GENERATED_DECL", CodeQueryKind::Definition);
    let mut results = vec![hit("src/context.ts", "context")];
    let best_score = results[0].score;

    append_definition_source_fallback(
        &status(),
        &request,
        &mut results,
        vec![
            SourceDeclarationMatch {
                path: "src/client.ts".to_owned(),
                excerpt: "export function RK_GENERATED_DECL() {}".to_owned(),
                byte_range: RepositoryCodeRange { start: 0, end: 36 },
                line_range: RepositoryCodeRange { start: 1, end: 1 },
                is_generated: false,
            },
            SourceDeclarationMatch {
                path: "dist/client.ts".to_owned(),
                excerpt: "export function RK_GENERATED_DECL() {}".to_owned(),
                byte_range: RepositoryCodeRange { start: 0, end: 36 },
                line_range: RepositoryCodeRange { start: 1, end: 1 },
                is_generated: true,
            },
        ],
    );

    let generated_score = score_for_path(&results, "dist/client.ts");
    let handwritten_score = score_for_path(&results, "src/client.ts");

    assert_eq!(handwritten_score, best_score + 4.0);
    assert!(generated_score > best_score);
    assert!(generated_score < handwritten_score);
}

fn score_for_path(results: &[CodeRetrievalHit], path: &str) -> f64 {
    results
        .iter()
        .find(|hit| hit.path == path)
        .unwrap_or_else(|| panic!("source fallback hit should remain for {path}"))
        .score
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
