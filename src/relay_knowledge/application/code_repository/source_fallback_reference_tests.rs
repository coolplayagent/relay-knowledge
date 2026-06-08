use super::*;
use crate::domain::{RepositoryCodeRange, code_snapshot_scope_id};

#[test]
fn reference_fallback_uses_exact_file_filter_without_scope_path_lookup() {
    let request = request(
        "RK_TRACE_NOTE",
        CodeQueryKind::References,
        vec!["./src/driver_ops.c".to_owned()],
    );

    let plan = plan_code_grep_fallback(&status(), &request, &[])
        .expect("exact path filter should plan fallback");

    assert_eq!(plan.paths, ["src/driver_ops.c"]);
    assert!(!plan.needs_scope_paths());
}

#[test]
fn reference_fallback_extracts_leading_identity_from_multi_term_query() {
    let request = request(
        "FileSet typedef BySmallestKey LevelState added_files",
        CodeQueryKind::References,
        vec!["db/version_set.cc".to_owned()],
    );

    let plan = plan_code_grep_fallback(&status(), &request, &[])
        .expect("multi-term reference query should still plan bounded source recall");

    assert_eq!(plan.query, "FileSet");
    assert_eq!(plan.paths, ["db/version_set.cc"]);
    assert!(!plan.needs_scope_paths());
}

#[test]
fn reference_fallback_uses_scope_candidates_for_multi_term_query_without_hits() {
    let request = request(
        "FileSet typedef BySmallestKey LevelState added_files",
        CodeQueryKind::References,
        Vec::new(),
    );

    let plan = plan_code_grep_fallback(&status(), &request, &[])
        .expect("empty multi-term reference results should scan bounded scope candidates");

    assert_eq!(plan.query, "FileSet");
    assert!(plan.paths.is_empty());
    assert!(plan.needs_scope_paths());
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
        exclude_generated: false,
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
                is_generated: false,
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
                is_generated: false,
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
fn reference_grep_fallback_ranks_declaration_first_for_typedef_intent() {
    let request = request(
        "FileSet typedef BySmallestKey LevelState added_files",
        CodeQueryKind::References,
        vec!["db/version_set.cc".to_owned()],
    );
    let plan = CodeGrepFallbackPlan {
        commit: "commit".to_owned(),
        query: "FileSet".to_owned(),
        paths: vec!["db/version_set.cc".to_owned()],
        path_filters: vec!["db/version_set.cc".to_owned()],
        language_filters: vec!["cpp".to_owned()],
        limit: 5,
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
                    path: "db/version_set.cc".to_owned(),
                    language_id: "cpp".to_owned(),
                    excerpt: "FileSet* added_files;".to_owned(),
                    byte_range: RepositoryCodeRange { start: 10, end: 30 },
                    line_range: RepositoryCodeRange {
                        start: 589,
                        end: 589,
                    },
                    is_generated: false,
                },
                SourceGrepMatch {
                    path: "db/version_set.cc".to_owned(),
                    language_id: "cpp".to_owned(),
                    excerpt: "typedef std::set<FileMetaData*, BySmallestKey> FileSet;".to_owned(),
                    byte_range: RepositoryCodeRange { start: 40, end: 96 },
                    line_range: RepositoryCodeRange {
                        start: 586,
                        end: 586,
                    },
                    is_generated: false,
                },
            ],
            degraded_reason: None,
        },
    );

    assert!(results[0].excerpt.starts_with("typedef std::set"));
    assert!(results[0].score > results[1].score);
}

#[test]
fn reference_grep_fallback_keeps_assignment_values_at_base_score() {
    assert_eq!(
        reference_source_grep_score_adjustment(
            "rk_driver_read",
            "rk_driver_read",
            ".read = rk_driver_read,"
        ),
        0.0
    );
    assert_eq!(
        reference_source_grep_score_adjustment(
            "rk_driver_read",
            "rk_driver_read",
            "return rk_driver_read;"
        ),
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
