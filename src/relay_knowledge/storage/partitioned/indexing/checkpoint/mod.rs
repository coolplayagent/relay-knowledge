//! Checkpoint lookup routed by recorded source scope or repository shard.

use crate::{
    domain::CodeIndexCheckpoint,
    storage::{CodeRepositoryStore, StorageFuture},
};

use super::super::PartitionedSqliteKnowledgeStore;

pub(in crate::storage::partitioned) fn by_scope(
    store: &PartitionedSqliteKnowledgeStore,
    source_scope: String,
) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .checkpoint_scope_store(source_scope.clone())
            .await?
        {
            if let Some(checkpoint) = shard.code_index_checkpoint(source_scope.clone()).await? {
                return Ok(Some(checkpoint));
            }
        }
        store.control.code_index_checkpoint(source_scope).await
    })
}

pub(in crate::storage::partitioned) fn latest(
    store: &PartitionedSqliteKnowledgeStore,
    repository_id: String,
) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .checkpoint_repository_store(repository_id.clone())
            .await?
        {
            return shard.latest_code_index_checkpoint(repository_id).await;
        }
        store
            .control
            .latest_code_index_checkpoint(repository_id)
            .await
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
