//! Import source-fallback completeness contracts.

use super::super::plan::plan_code_grep_fallback;
use crate::domain::{
    CodeQueryKind, CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalLayer,
    CodeRetrievalRequest, FreshnessPolicy, RepositoryCodeRange, code_snapshot_scope_id,
};

#[test]
fn complete_unresolved_external_import_statements_skip_source_search() {
    for (query, excerpt) in [
        ("okio.Sink", "import okio.Sink"),
        (
            "Illuminate\\Container\\Container",
            "use Illuminate\\Container\\Container;",
        ),
        ("Foundation", "import Foundation"),
    ] {
        let hit = unresolved_import_hit("src/client.ext", excerpt, excerpt);
        assert!(
            plan_code_grep_fallback(&status(), &request(query), &[hit]).is_none(),
            "{excerpt} should already provide a complete source surface"
        );
    }
}

#[test]
fn incomplete_or_mixed_external_import_surfaces_still_plan_bounded_repair() {
    let complete = unresolved_import_hit(
        "src/client.ts",
        "import Client from \"client-sdk\";",
        "import Client from \"client-sdk\";",
    );
    let incomplete = unresolved_import_hit("src/legacy.ts", "client-sdk", "client-sdk");

    let plan = plan_code_grep_fallback(&status(), &request("Client"), &[complete, incomplete])
        .expect("one incomplete graph surface should retain bounded source repair");

    assert_eq!(plan.query, "client-sdk");
    assert_eq!(plan.paths, ["src/client.ts", "src/legacy.ts"]);
    assert!(!plan.needs_scope_paths());
}

#[test]
fn dynamic_import_intent_keeps_source_search_even_with_static_graph_surface() {
    let hit = unresolved_import_hit(
        "src/client.ts",
        "import Client from \"client-sdk\";",
        "import Client from \"client-sdk\";",
    );

    assert!(
        plan_code_grep_fallback(&status(), &request("import(\"client-sdk\")"), &[hit]).is_some()
    );
}

#[test]
fn relative_import_queries_keep_bounded_scope_search() {
    let hit = unresolved_import_hit(
        "src/client.ts",
        "import Client from \"./client\";",
        "import Client from \"./client\";",
    );

    let plan = plan_code_grep_fallback(&status(), &request("./client"), &[hit])
        .expect("relative imports still need source resolution across the authorized scope");

    assert_eq!(plan.query, "./client");
    assert!(plan.paths.is_empty());
    assert!(plan.needs_scope_paths());
}

fn request(query: &str) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        CodeQueryKind::Imports,
        20,
        FreshnessPolicy::AllowStale,
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
        indexed_file_count: 2,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 2,
        stale: false,
        degraded_reason: None,
    }
}

fn unresolved_import_hit(
    path: &str,
    target_hint: &str,
    excerpt: &str,
) -> crate::domain::CodeRetrievalHit {
    crate::domain::CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 0 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: Some(format!("file:{path}")),
        retrieval_layers: vec![CodeRetrievalLayer::ImportGraph],
        index_versions: Vec::new(),
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: Some("import".to_owned()),
        edge_resolution_state: Some("unresolved".to_owned()),
        edge_target_hint: Some(target_hint.to_owned()),
        edge_confidence_basis_points: Some(2_500),
        edge_confidence_tier: Some("ambiguous".to_owned()),
        score: 3.0,
        excerpt: excerpt.to_owned(),
    }
}
