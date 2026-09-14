//! Checkpoint lookup routed by recorded source scope or repository shard.

use crate::{
    domain::CodeIndexCheckpoint,
    storage::{CodeIndexPublicationStore, StorageFuture},
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
            if let Some(mut checkpoint) = shard.code_index_checkpoint(source_scope.clone()).await? {
                let active = store
                    .catalog
                    .active_repository_for_scope(source_scope.clone())
                    .await?
                    .as_deref()
                    == Some(checkpoint.repository_id.as_str());
                let query_indexes_ready = shard.code_query_indexes_ready_for_publication().await?;
                project_publication_state(&mut checkpoint, active, query_indexes_ready);
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
            let Some(mut checkpoint) = shard.latest_code_index_checkpoint(repository_id).await?
            else {
                return Ok(None);
            };
            let active = store
                .catalog
                .active_repository_for_scope(checkpoint.source_scope.clone())
                .await?
                .as_deref()
                == Some(checkpoint.repository_id.as_str());
            let query_indexes_ready = shard.code_query_indexes_ready_for_publication().await?;
            project_publication_state(&mut checkpoint, active, query_indexes_ready);
            return Ok(Some(checkpoint));
        }
        store
            .control
            .latest_code_index_checkpoint(repository_id)
            .await
    })
}

pub(super) fn project_publication_state(
    checkpoint: &mut CodeIndexCheckpoint,
    active: bool,
    query_indexes_ready: bool,
) {
    if checkpoint.state == "completed" && !active {
        checkpoint.state = "finalizing:partitioned_publish".to_owned();
    } else if checkpoint.state == "finalizing:partitioned_publish" && active && query_indexes_ready
    {
        checkpoint.state = "completed".to_owned();
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
