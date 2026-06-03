use std::collections::BTreeSet;

use crate::domain::CodeScopeRetentionSummary;

pub(super) fn merge_scope_retention_summaries(
    repository_id: String,
    control: CodeScopeRetentionSummary,
    shard: CodeScopeRetentionSummary,
) -> CodeScopeRetentionSummary {
    let retained_scopes = union_scopes([control.retained_scopes, shard.retained_scopes]);
    let prunable_scopes = union_scopes([control.prunable_scopes, shard.prunable_scopes]);
    let pruned_scopes = union_scopes([control.pruned_scopes, shard.pruned_scopes]);

    CodeScopeRetentionSummary {
        repository_id,
        retained_scope_count: retained_scopes.len(),
        prunable_scope_count: prunable_scopes.len(),
        pruned_scope_count: pruned_scopes.len(),
        retained_scopes,
        prunable_scopes,
        pruned_scopes,
    }
}

fn union_scopes(scopes: impl IntoIterator<Item = Vec<String>>) -> Vec<String> {
    scopes
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
