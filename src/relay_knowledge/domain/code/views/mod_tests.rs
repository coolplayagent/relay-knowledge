//! Direct contracts for bounded codebase-view requests.

use super::*;

#[test]
fn view_request_allows_changed_paths_beyond_output_limit() {
    let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap();

    let request = CodebaseViewRequest::new(
        selector,
        CodebaseViewKind::AffectedScope,
        FreshnessPolicy::AllowStale,
        1,
        vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()],
    )
    .unwrap();

    assert_eq!(request.changed_paths.len(), 2);
}

#[test]
fn view_request_rejects_changed_paths_beyond_input_cap() {
    let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap();
    let changed_paths = (0..=MAX_CODEBASE_VIEW_CHANGED_PATHS)
        .map(|index| format!("src/{index}.rs"))
        .collect();

    let error = CodebaseViewRequest::new(
        selector,
        CodebaseViewKind::AffectedScope,
        FreshnessPolicy::AllowStale,
        100,
        changed_paths,
    )
    .unwrap_err();

    assert!(error.to_string().contains("changed_paths"));
}
