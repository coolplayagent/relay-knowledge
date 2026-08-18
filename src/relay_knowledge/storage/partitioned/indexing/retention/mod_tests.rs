//! Tests the retention owner after its move under partitioned indexing.

use super::{merge_scope_retention_summaries, shard_retained_pins};
use crate::domain::CodeScopeRetentionSummary;

#[test]
fn retention_merge_deduplicates_and_sorts_each_scope_class() {
    let merged = merge_scope_retention_summaries(
        "repo".to_owned(),
        summary(vec!["scope-b", "scope-a"], vec!["scope-c"], vec!["scope-d"]),
        summary(vec!["scope-a"], vec!["scope-b", "scope-c"], vec!["scope-d"]),
    );

    assert_eq!(merged.repository_id, "repo");
    assert_eq!(merged.retained_scopes, ["scope-a", "scope-b"]);
    assert_eq!(merged.prunable_scopes, ["scope-b", "scope-c"]);
    assert_eq!(merged.pruned_scopes, ["scope-d"]);
    assert_eq!(merged.retained_scope_count, 2);
    assert_eq!(merged.prunable_scope_count, 2);
    assert_eq!(merged.pruned_scope_count, 1);
    assert!(!merged.maintenance_pending);
}

#[test]
fn retention_merge_reports_truncated_counts_as_nonduplicating_lower_bounds() {
    let mut control = summary(vec!["scope-a"], vec![], vec![]);
    control.retained_scope_count = 64;
    control.scope_listing_truncated = true;
    let mut shard = summary(vec!["scope-a", "scope-b"], vec![], vec![]);
    shard.retained_scope_count = 64;
    shard.scope_listing_truncated = true;

    let merged = merge_scope_retention_summaries("repo".to_owned(), control, shard);

    assert_eq!(merged.retained_scopes, ["scope-a", "scope-b"]);
    assert_eq!(merged.retained_scope_count, 64);
    assert!(merged.scope_listing_truncated);
}

#[test]
fn truncated_control_pins_pause_partitioned_shard_retirement() {
    let mut control = summary(vec!["scope-a"], vec![], vec![]);
    control.retained_scope_count = 513;
    control.scope_listing_truncated = true;

    let error = shard_retained_pins(&control)
        .expect_err("a partial control pin projection must not drive shard deletion");

    assert!(error.to_string().contains("shard maintenance is paused"));
}

fn summary(
    retained: Vec<&str>,
    prunable: Vec<&str>,
    pruned: Vec<&str>,
) -> CodeScopeRetentionSummary {
    CodeScopeRetentionSummary {
        repository_id: "ignored".to_owned(),
        retained_scope_count: 0,
        prunable_scope_count: 0,
        pruned_scope_count: 0,
        scope_listing_truncated: false,
        retiring_job_count: 0,
        maintenance_pending: false,
        retained_scopes: strings(retained),
        prunable_scopes: strings(prunable),
        pruned_scopes: strings(pruned),
        retiring_jobs: Vec::new(),
        repository_retention_job: None,
    }
}

fn strings(values: Vec<&str>) -> Vec<String> {
    values.into_iter().map(str::to_owned).collect()
}
