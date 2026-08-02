//! Scope retention routing and deterministic control/shard summary merging.

use std::collections::BTreeSet;

use crate::{
    domain::CodeScopeRetentionSummary,
    storage::{CodeRepositoryStore, CodeScopeRetentionRequest, StorageFuture},
};

use super::super::PartitionedSqliteKnowledgeStore;

pub(in crate::storage::partitioned) fn status(
    store: &PartitionedSqliteKnowledgeStore,
    repository_id: String,
) -> StorageFuture<'_, CodeScopeRetentionSummary> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .existing_repository_store(repository_id.clone())
            .await?
        {
            return shard.code_scope_retention(repository_id).await;
        }
        store.control.code_scope_retention(repository_id).await
    })
}

pub(in crate::storage::partitioned) fn prune(
    store: &PartitionedSqliteKnowledgeStore,
    request: CodeScopeRetentionRequest,
) -> StorageFuture<'_, CodeScopeRetentionSummary> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .existing_repository_store(request.repository_id.clone())
            .await?
        {
            let control_retention = store
                .control
                .prune_code_repository_scopes(request.clone())
                .await?;
            let shard_retention = shard
                .prune_code_repository_scopes_with_retained(
                    request.clone(),
                    control_retention.retained_scopes.clone(),
                )
                .await;
            return shard_retention.map(|summary| {
                merge_scope_retention_summaries(request.repository_id, control_retention, summary)
            });
        }
        store.control.prune_code_repository_scopes(request).await
    })
}

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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
