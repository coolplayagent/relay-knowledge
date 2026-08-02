//! Tests the retention owner after its move under partitioned indexing.

use super::merge_scope_retention_summaries;
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
        retained_scopes: strings(retained),
        prunable_scopes: strings(prunable),
        pruned_scopes: strings(pruned),
    }
}

fn strings(values: Vec<&str>) -> Vec<String> {
    values.into_iter().map(str::to_owned).collect()
}
