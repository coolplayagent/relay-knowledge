use super::{
    CodeIndexResourceBudget, CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest,
    FreshnessPolicy, RepositoryCodeRange, code_snapshot_scope_id,
};

#[test]
fn selector_trims_and_deduplicates_filters() {
    let selector = CodeRepositorySelector::new(
        " repo ",
        " HEAD ",
        vec!["src".to_owned(), " src ".to_owned()],
        vec!["rust".to_owned(), "rust".to_owned()],
    )
    .expect("selector should validate");

    assert_eq!(selector.repository, "repo");
    assert_eq!(selector.ref_selector, "HEAD");
    assert_eq!(selector.path_filters, ["src"]);
    assert_eq!(selector.language_filters, ["rust"]);
}

#[test]
fn snapshot_scope_id_tracks_tree_and_filters() {
    let scope = code_snapshot_scope_id(
        "repo-1",
        "tree-a",
        &["src".to_owned()],
        &["rust".to_owned()],
    );
    let same = code_snapshot_scope_id(
        "repo-1",
        "tree-a",
        &["src".to_owned()],
        &["rust".to_owned()],
    );
    let different_tree = code_snapshot_scope_id(
        "repo-1",
        "tree-b",
        &["src".to_owned()],
        &["rust".to_owned()],
    );

    assert_eq!(scope, same);
    assert_ne!(scope, different_tree);
    assert!(scope.starts_with("git_snapshot:"));
}

#[test]
fn retrieval_request_rejects_unbounded_limits() {
    let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");
    let error = CodeRetrievalRequest::new(
        "symbol",
        selector,
        CodeQueryKind::Hybrid,
        51,
        FreshnessPolicy::AllowStale,
    )
    .expect_err("large limit should fail");

    assert_eq!(error.field, "limit");
}

#[test]
fn code_ranges_must_be_ordered() {
    let error = RepositoryCodeRange::new("line_range", 3, 2).expect_err("range should fail");

    assert_eq!(error.field, "line_range");
}

#[test]
fn default_code_index_budget_batches_more_small_files_without_raising_row_or_byte_caps() {
    let budget = CodeIndexResourceBudget::default();

    assert_eq!(budget.max_files_per_batch, 256);
    assert_eq!(
        budget.max_bytes_per_batch,
        CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH
    );
    assert_eq!(
        budget.max_rows_per_batch,
        CodeIndexResourceBudget::DEFAULT_MAX_ROWS_PER_BATCH
    );
}
