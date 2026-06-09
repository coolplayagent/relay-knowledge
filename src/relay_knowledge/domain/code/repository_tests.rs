use super::{
    CodeIndexProgressSummary, CodeIndexResourceBudget, CodeIndexSummary, CodeQueryKind,
    CodeRepositoryReport, CodeRepositorySelector, CodeRepositoryTotals, CodeRetrievalRequest,
    FreshnessPolicy, RepositoryCodeRange, code_snapshot_expected_scope_id, code_snapshot_scope_id,
    code_snapshot_scope_is_fact_versioned,
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
fn expected_snapshot_scope_id_is_checked_for_unfiltered_repositories() {
    let scope = code_snapshot_scope_id("repo-1", "tree-a", &[], &[]);
    let expected = code_snapshot_expected_scope_id("repo-1", "tree-a", &[], &[])
        .expect("all repository snapshots should carry a fact version");

    assert_eq!(expected, scope);
}

#[test]
fn fact_versioned_snapshot_scope_requires_generated_hash_shape() {
    assert!(code_snapshot_scope_is_fact_versioned(
        "git_snapshot:0123456789abcdef"
    ));
    assert!(!code_snapshot_scope_is_fact_versioned("git_snapshot:test"));
    assert!(!code_snapshot_scope_is_fact_versioned("manual:test"));
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

    assert_eq!(budget.max_files_per_batch, 512);
    assert_eq!(
        budget.max_bytes_per_batch,
        CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH
    );
    assert_eq!(
        budget.max_rows_per_batch,
        CodeIndexResourceBudget::DEFAULT_MAX_ROWS_PER_BATCH
    );
}

#[test]
fn generated_count_fields_default_when_deserializing_older_responses() {
    let mut summary_json = serde_json::to_value(CodeIndexSummary {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        indexed_file_count: 1,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_path_count: 0,
        symbol_count: 2,
        handwritten_symbol_count: 1,
        generated_symbol_count: 1,
        reference_count: 0,
        chunk_count: 0,
        degraded_file_count: 0,
        progress: CodeIndexProgressSummary::default(),
    })
    .expect("summary should serialize");
    let summary_object = summary_json
        .as_object_mut()
        .expect("summary json should be an object");
    summary_object.remove("handwritten_symbol_count");
    summary_object.remove("generated_symbol_count");
    let summary = serde_json::from_value::<CodeIndexSummary>(summary_json)
        .expect("older summary response should deserialize");

    assert_eq!(summary.handwritten_symbol_count, 0);
    assert_eq!(summary.generated_symbol_count, 0);

    let mut totals_json =
        serde_json::to_value(CodeRepositoryTotals::default()).expect("totals should serialize");
    let totals_object = totals_json
        .as_object_mut()
        .expect("totals json should be an object");
    totals_object.remove("handwritten_symbol_count");
    totals_object.remove("generated_symbol_count");
    let totals = serde_json::from_value::<CodeRepositoryTotals>(totals_json)
        .expect("older totals response should deserialize");

    assert_eq!(totals.handwritten_symbol_count, 0);
    assert_eq!(totals.generated_symbol_count, 0);

    let mut report_json = serde_json::to_value(CodeRepositoryReport {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        resolved_commit_sha: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        indexed_file_count: 1,
        symbol_count: 2,
        handwritten_symbol_count: 1,
        generated_symbol_count: 1,
        reference_count: 0,
        chunk_count: 0,
        degraded_file_count: 0,
        resolved_edge_count: 0,
        ambiguous_edge_count: 0,
        unresolved_edge_count: 0,
        degradation_summary: Vec::new(),
        representative_queries: Vec::new(),
        latency_samples: Vec::new(),
        freshness_state: "fresh".to_owned(),
    })
    .expect("report should serialize");
    let report_object = report_json
        .as_object_mut()
        .expect("report json should be an object");
    report_object.remove("handwritten_symbol_count");
    report_object.remove("generated_symbol_count");
    let report = serde_json::from_value::<CodeRepositoryReport>(report_json)
        .expect("older report response should deserialize");

    assert_eq!(report.handwritten_symbol_count, 0);
    assert_eq!(report.generated_symbol_count, 0);
}
