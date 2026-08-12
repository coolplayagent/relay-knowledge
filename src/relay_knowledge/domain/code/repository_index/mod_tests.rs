//! Direct contracts for repository-index budgets and summaries.

use super::{
    CodeIndexProgressSummary, CodeIndexResourceBudget, CodeIndexSummary, CodeScopeRetentionSummary,
};

#[test]
fn default_budget_batches_more_small_files_without_raising_row_or_byte_caps() {
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
fn scope_retention_gc_status_defaults_when_deserializing_older_responses() {
    let summary = serde_json::from_value::<CodeScopeRetentionSummary>(serde_json::json!({
        "repository_id": "repo",
        "retained_scope_count": 1,
        "prunable_scope_count": 0,
        "pruned_scope_count": 0,
        "retained_scopes": ["scope"],
        "prunable_scopes": [],
        "pruned_scopes": []
    }))
    .expect("older retention response should deserialize");

    assert_eq!(summary.retiring_job_count, 0);
    assert!(!summary.maintenance_pending);
    assert!(summary.retiring_jobs.is_empty());
    assert!(!summary.scope_listing_truncated);
}

#[test]
fn generated_summary_counts_default_when_deserializing_older_responses() {
    let mut summary_json = serde_json::to_value(CodeIndexSummary {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: Some("base".to_owned()),
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
    summary_object.remove("base_resolved_commit_sha");
    let summary = serde_json::from_value::<CodeIndexSummary>(summary_json)
        .expect("older summary response should deserialize");

    assert_eq!(summary.handwritten_symbol_count, 0);
    assert_eq!(summary.generated_symbol_count, 0);
    assert_eq!(summary.base_resolved_commit_sha, None);
}
