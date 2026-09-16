use super::*;
use crate::code::search::{SourceGrepKind, SourceGrepRequest};

#[test]
fn rejected_candidates_still_consume_the_bounded_scan_budget() {
    let filters = crate::domain::code_scope_language_filters(&["c".into()], &["cpp".into()]);
    let request = SourceGrepRequest {
        query: "target".into(),
        paths: (0..257).map(|i| format!("src/a{i}.c")).collect(),
        path_filters: vec![],
        language_filters: filters,
        limit: 10,
        kind: SourceGrepKind::References,
        exclude_generated: false,
    };
    let selected = selected_candidate_paths(&request);
    assert!(selected.paths.is_empty());
    assert!(
        selected
            .degraded_reason
            .unwrap()
            .contains("budget exhausted")
    );
}

#[test]
fn unknown_language_filter_allows_document_source_fallback_candidates() {
    assert!(language_filter_allows(
        "docs/operations.md",
        "markdown",
        &["unknown".to_owned()]
    ));
    assert!(!language_filter_allows(
        "src/service.py",
        "python",
        &["unknown".to_owned()]
    ));
}

#[test]
fn candidate_paths_apply_scope_filters_and_budget() {
    let request = SourceGrepRequest {
        query: "target".to_owned(),
        paths: vec![
            "src/lib.rs".to_owned(),
            "../bad.rs".to_owned(),
            "tests/lib.rs".to_owned(),
            "src/app.py".to_owned(),
        ],
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        limit: 5,
        kind: SourceGrepKind::Hybrid,
        exclude_generated: false,
    };

    let candidates = selected_candidate_paths(&request);

    assert_eq!(candidates.paths, ["src/lib.rs"]);
}

#[test]
fn candidate_paths_exclude_generated_paths_when_requested() {
    let request = SourceGrepRequest {
        query: "target".to_owned(),
        paths: vec!["dist/bundle.js".to_owned(), "src/lib.rs".to_owned()],
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        limit: 5,
        kind: SourceGrepKind::Hybrid,
        exclude_generated: true,
    };

    let candidates = selected_candidate_paths(&request);

    assert_eq!(candidates.paths, ["src/lib.rs"]);
}

#[test]
fn candidate_match_pool_reserves_two_lines_per_bounded_path() {
    assert_eq!(bounded_source_grep_candidate_match_limit(20, 256), 512);
    assert_eq!(bounded_source_grep_candidate_match_limit(50, 1), 50);
    assert_eq!(bounded_source_grep_candidate_match_limit(20, 257), 512);
}
