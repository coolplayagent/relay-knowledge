use super::*;
use crate::domain::{RepositoryCodeRange, code_snapshot_scope_id};

#[test]
fn fallback_plan_uses_contextual_hits_and_exact_file_filters() {
    let request = request(
        "rk_read_fn",
        CodeQueryKind::Definition,
        vec!["include/driver_ops.h".to_owned()],
    );
    let hit = hit(
        "include/driver_ops.h",
        "struct rk_driver_ops {\n    rk_read_fn read;\n}",
    );

    let plan = plan_code_grep_fallback(&status(), &request, &[hit])
        .expect("contextual hit should plan fallback");

    assert_eq!(plan.identity.as_deref(), Some("rk_read_fn"));
    assert_eq!(plan.query, "rk_read_fn");
    assert_eq!(plan.paths, ["include/driver_ops.h"]);
}

#[test]
fn fallback_plan_skips_results_with_exact_declaration() {
    let request = request("rk_read_fn", CodeQueryKind::Definition, Vec::new());
    let mut hit = hit(
        "include/driver_ops.h",
        "typedef int (*rk_read_fn)(struct rk_device *dev);",
    );
    hit.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];

    assert!(plan_code_grep_fallback(&status(), &request, &[hit]).is_none());
}

#[test]
fn hybrid_grep_fallback_fills_after_structured_hits() {
    let request = request("rk_helper", CodeQueryKind::Hybrid, Vec::new());
    let mut results = vec![hit("src/lib.c", "void structured_hit(void);")];
    let plan = plan_code_grep_fallback(&status(), &request, &results)
        .expect("partial hybrid results should plan fallback");
    let outcome = SourceGrepOutcome {
        matches: vec![SourceGrepMatch {
            path: "src/fallback.c".to_owned(),
            language_id: "c".to_owned(),
            excerpt: "rk_helper();".to_owned(),
            byte_range: RepositoryCodeRange { start: 10, end: 19 },
            line_range: RepositoryCodeRange { start: 4, end: 4 },
        }],
        degraded_reason: None,
    };

    append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome);

    assert_eq!(results[0].path, "src/lib.c");
    let fallback = results
        .iter()
        .find(|hit| hit.path == "src/fallback.c")
        .expect("fallback hit should be appended");
    assert!(fallback.score < results[0].score);
    assert!(
        fallback
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[test]
fn hybrid_grep_fallback_skips_exact_symbol_coverage() {
    let request = request("ConnectorService", CodeQueryKind::Hybrid, Vec::new());
    let mut result = hit("src/service.py", "class ConnectorService:");
    result.language_id = "python".to_owned();
    result.retrieval_layers = vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition];
    result.canonical_symbol_id =
        Some("repo://repo/src::relay_teams::connector::service::ConnectorService".to_owned());

    assert!(plan_code_grep_fallback(&status(), &request, &[result]).is_none());
}

#[test]
fn hybrid_grep_fallback_uses_text_fallback_for_non_symbol_coverage() {
    let request = request("ConnectorService", CodeQueryKind::Hybrid, Vec::new());
    let result = hit(
        "docs/service.md",
        "ConnectorService appears in deployment notes.",
    );

    let plan = plan_code_grep_fallback(&status(), &request, &[result])
        .expect("lexical-only context should still allow source fallback");

    assert_eq!(plan.kind, SourceGrepKind::Hybrid);
    assert!(plan.needs_scope_paths());
}

#[test]
fn import_fallback_runs_for_unresolved_external_imports_and_reports_capability() {
    let request = request("ProviderShared", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("src/component.tsx", "react");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("unresolved".to_owned());
    import_hit.edge_target_hint = Some("react".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let mut results = vec![import_hit];
    let plan = plan_code_grep_fallback(&status(), &request, &results)
        .expect("unresolved import should plan source fallback");
    assert_eq!(plan.query, "react");
    assert_eq!(plan.paths, ["src/component.tsx"]);
    assert!(!plan.needs_scope_paths());
    let outcome = SourceGrepOutcome {
        matches: vec![SourceGrepMatch {
            path: "src/component.tsx".to_owned(),
            language_id: "tsx".to_owned(),
            excerpt: "import React from \"react\";".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 26 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
        }],
        degraded_reason: None,
    };

    let reason = append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome)
        .expect("import fallback should explain external dependency fallback");

    assert!(reason.contains("external dependency import is not indexed"));
    assert!(results.iter().any(|hit| {
        hit.retrieval_layers
            .contains(&CodeRetrievalLayer::ImportGraph)
    }));
    assert!(results.iter().any(|hit| {
        hit.retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    }));
}

#[test]
fn import_fallback_searches_only_matching_unresolved_external_import_paths() {
    let request = request("ProviderShared", CodeQueryKind::Imports, Vec::new());
    let mut react_one = hit("src/component.tsx", "react");
    react_one.edge_kind = Some("import".to_owned());
    react_one.edge_resolution_state = Some("unresolved".to_owned());
    react_one.edge_target_hint = Some("import React from \"react\";".to_owned());
    react_one.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let mut vue = hit("src/other.tsx", "vue");
    vue.edge_kind = Some("import".to_owned());
    vue.edge_resolution_state = Some("unresolved".to_owned());
    vue.edge_target_hint = Some("import Vue from \"vue\";".to_owned());
    vue.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let mut react_two = hit("src/nested/panel.tsx", "react");
    react_two.edge_kind = Some("import".to_owned());
    react_two.edge_resolution_state = Some("unresolved".to_owned());
    react_two.edge_target_hint = Some("react".to_owned());
    react_two.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    let plan = plan_code_grep_fallback(&status(), &request, &[react_one, vue, react_two])
        .expect("unresolved external imports should plan fallback");

    assert_eq!(plan.query, "react");
    assert_eq!(plan.paths, ["src/component.tsx", "src/nested/panel.tsx"]);
    assert!(!plan.needs_scope_paths());
}

#[test]
fn import_fallback_keeps_graph_evidence_ahead_of_text_fallback() {
    let mut request = request("ProviderShared", CodeQueryKind::Imports, Vec::new());
    request.limit = 1;
    let mut import_hit = hit("src/component.tsx", "react");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("unresolved".to_owned());
    import_hit.edge_target_hint = Some("react".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];
    let plan = plan_code_grep_fallback(&status(), &request, &[import_hit.clone()])
        .expect("unresolved import should plan source fallback");
    let mut results = vec![import_hit];
    let outcome = SourceGrepOutcome {
        matches: vec![SourceGrepMatch {
            path: "src/component.tsx".to_owned(),
            language_id: "tsx".to_owned(),
            excerpt: "import React from \"react\";".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 26 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
        }],
        degraded_reason: None,
    };

    append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].excerpt, "react");
    assert_eq!(
        results[0].edge_resolution_state.as_deref(),
        Some("unresolved")
    );
    assert!(
        results[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::ImportGraph)
    );
}

#[test]
fn import_fallback_skips_empty_import_results() {
    let request = request("react", CodeQueryKind::Imports, Vec::new());

    assert!(plan_code_grep_fallback(&status(), &request, &[]).is_none());
}

#[test]
fn import_fallback_skips_resolved_import_graph_hits() {
    let request = request("crate::local", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("src/lib.rs", "use crate::local;");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("resolved".to_owned());
    import_hit.edge_target_hint = Some("crate::local".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
}

#[test]
fn import_fallback_skips_ambiguous_import_graph_hits() {
    let request = request("RetryPolicy", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("src/app.rs", "use app::RetryPolicy;");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("ambiguous".to_owned());
    import_hit.edge_target_hint = Some("app::RetryPolicy".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
}

#[test]
fn import_fallback_skips_local_unresolved_import_graph_hits() {
    let request = request("crate::local", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("src/lib.rs", "use crate::local;");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("unresolved".to_owned());
    import_hit.edge_target_hint = Some("use crate::local;".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
}

#[test]
fn import_fallback_skips_dot_prefixed_local_unresolved_import_graph_hits() {
    let request = request(".pkg", CodeQueryKind::Imports, Vec::new());
    let mut import_hit = hit("pkg/app.py", "from .pkg import service");
    import_hit.edge_kind = Some("import".to_owned());
    import_hit.edge_resolution_state = Some("unresolved".to_owned());
    import_hit.edge_target_hint = Some("from .pkg import service".to_owned());
    import_hit.retrieval_layers = vec![CodeRetrievalLayer::ImportGraph];

    assert!(plan_code_grep_fallback(&status(), &request, &[import_hit]).is_none());
}

#[test]
fn reference_grep_fallback_ranks_usage_before_array_declaration() {
    let request = request("rk_pipeline", CodeQueryKind::References, Vec::new());
    let mut results = vec![hit("src/pipeline.c", "int rk_dispatch(void);")];
    let plan = CodeGrepFallbackPlan {
        commit: "commit".to_owned(),
        query: "rk_pipeline".to_owned(),
        paths: Vec::new(),
        path_filters: Vec::new(),
        language_filters: vec!["c".to_owned()],
        limit: 10,
        kind: SourceGrepKind::References,
        identity: None,
        needs_scope_paths: false,
    };
    let outcome = SourceGrepOutcome {
        matches: vec![
            SourceGrepMatch {
                path: "src/pipeline.c".to_owned(),
                language_id: "c".to_owned(),
                excerpt: "static rk_stage_fn rk_pipeline[] = {".to_owned(),
                byte_range: RepositoryCodeRange { start: 10, end: 48 },
                line_range: RepositoryCodeRange { start: 4, end: 4 },
            },
            SourceGrepMatch {
                path: "src/pipeline.c".to_owned(),
                language_id: "c".to_owned(),
                excerpt: "total += rk_pipeline[index](dev);".to_owned(),
                byte_range: RepositoryCodeRange {
                    start: 90,
                    end: 123,
                },
                line_range: RepositoryCodeRange { start: 9, end: 9 },
            },
        ],
        degraded_reason: Some("source fallback".to_owned()),
    };

    append_code_grep_fallback(&status(), &request, &mut results, &plan, outcome);

    let usage_rank = results
        .iter()
        .position(|hit| hit.excerpt.contains("rk_pipeline[index]"))
        .expect("usage fallback should be returned");
    let declaration_rank = results
        .iter()
        .position(|hit| hit.excerpt.contains("rk_pipeline[]"))
        .expect("declaration fallback should be returned");
    assert!(usage_rank < declaration_rank);
    assert!(results[usage_rank].score > results[declaration_rank].score);
}

#[test]
fn reference_grep_fallback_keeps_assignment_values_at_base_score() {
    assert_eq!(
        reference_source_grep_score_adjustment("rk_driver_read", ".read = rk_driver_read,"),
        0.0
    );
    assert_eq!(
        reference_source_grep_score_adjustment("rk_driver_read", "return rk_driver_read;"),
        0.0
    );
}

fn request(query: &str, kind: CodeQueryKind, path_filters: Vec<String>) -> CodeRetrievalRequest {
    let selector = crate::domain::CodeRepositorySelector::new(
        "repo",
        "commit",
        path_filters,
        vec!["c".to_owned()],
    )
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
        language_id: "c".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: Some("symbol".to_owned()),
        canonical_symbol_id: Some("repo://repo/include::driver_ops::rk_driver_ops".to_owned()),
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Lexical],
        index_versions: vec!["code:scope:tree".to_owned()],
        stale: false,
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
