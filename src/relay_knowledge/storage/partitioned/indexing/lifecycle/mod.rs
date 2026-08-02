//! Snapshot and checkpointed-session publication into repository shards.

use std::sync::Arc;

use crate::{
    domain::{
        CodeIndexBatch, CodeIndexCheckpoint, CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary,
    },
    storage::{CodeRepositoryStore, StorageError, StorageFuture},
};

use super::super::{
    PartitionedSqliteKnowledgeStore, routing::current_control_scope, status::mirror_status,
};

pub(in crate::storage::partitioned) fn apply_snapshot(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: CodeIndexSnapshot,
) -> StorageFuture<'_, CodeIndexSummary> {
    let store = store.clone();
    Box::pin(async move {
        let base_scope = incremental_base_scope(&store, &snapshot).await?;
        let shard = if snapshot.full_replace {
            store
                .catalog
                .staged_repository_store(snapshot.repository_id.clone())
                .await?
        } else {
            match store
                .catalog
                .existing_repository_store(snapshot.repository_id.clone())
                .await?
            {
                Some(shard) => shard,
                None => {
                    store
                        .catalog
                        .staged_repository_store(snapshot.repository_id.clone())
                        .await?
                }
            }
        };
        store
            .catalog
            .import_control_repository(
                Arc::clone(&shard),
                snapshot.repository_id.clone(),
                base_scope,
            )
            .await?;
        let summary = shard.apply_code_index_snapshot(snapshot).await?;
        publish_summary(&store, Arc::clone(&shard), &summary, "index").await?;
        Ok(summary)
    })
}

pub(in crate::storage::partitioned) fn clear_workspace(
    store: &PartitionedSqliteKnowledgeStore,
    repository_id: String,
    source_scope: String,
) -> StorageFuture<'_, ()> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .existing_repository_store(repository_id.clone())
            .await?
        {
            shard
                .clear_code_workspace_state(repository_id.clone(), source_scope.clone())
                .await?;
        }
        store
            .control
            .clear_code_workspace_state(repository_id, source_scope)
            .await
    })
}

pub(in crate::storage::partitioned) fn begin_session(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    let store = store.clone();
    Box::pin(async move {
        let repository_id = session.repository_id.clone();
        let source_scope = session.source_scope.clone();
        let shard = store
            .catalog
            .staged_repository_store(repository_id.clone())
            .await?;
        let control_scope = current_control_scope(&store.control, repository_id.clone()).await?;
        store
            .catalog
            .import_control_repository(Arc::clone(&shard), repository_id.clone(), control_scope)
            .await?;
        let checkpoint = shard.begin_code_index_session(session).await?;
        store
            .catalog
            .stage_scope(repository_id, source_scope)
            .await?;
        Ok(checkpoint)
    })
}

pub(in crate::storage::partitioned) fn apply_batch(
    store: &PartitionedSqliteKnowledgeStore,
    batch: CodeIndexBatch,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    let store = store.clone();
    Box::pin(async move {
        let repository_id = batch.repository_id.clone();
        let source_scope = batch.source_scope.clone();
        let shard = store
            .catalog
            .staged_repository_store(repository_id.clone())
            .await?;
        let control_scope = current_control_scope(&store.control, repository_id.clone()).await?;
        store
            .catalog
            .import_control_repository(Arc::clone(&shard), repository_id.clone(), control_scope)
            .await?;
        let checkpoint = shard.apply_code_index_batch(batch).await?;
        store
            .catalog
            .stage_scope(repository_id, source_scope)
            .await?;
        Ok(checkpoint)
    })
}

pub(in crate::storage::partitioned) fn finalize_session(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexSummary> {
    let store = store.clone();
    Box::pin(async move {
        let shard = store
            .catalog
            .staged_repository_store(session.repository_id.clone())
            .await?;
        let summary = shard.finalize_code_index_session(session).await?;
        publish_summary(&store, shard, &summary, "finalize").await?;
        Ok(summary)
    })
}

async fn incremental_base_scope(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: &CodeIndexSnapshot,
) -> Result<Option<String>, StorageError> {
    if snapshot.full_replace {
        return Ok(None);
    }
    let Some(base_commit) = snapshot.base_resolved_commit_sha.clone() else {
        return Ok(None);
    };

    Ok(store
        .control
        .code_repository_scope_status(
            snapshot.repository_id.clone(),
            base_commit,
            snapshot.path_filters.clone(),
            snapshot.language_filters.clone(),
        )
        .await?
        .and_then(|status| status.last_indexed_scope_id))
}

async fn publish_summary(
    store: &PartitionedSqliteKnowledgeStore,
    shard: Arc<crate::storage::SqliteGraphStore>,
    summary: &CodeIndexSummary,
    stage: &'static str,
) -> Result<(), StorageError> {
    let status = shard
        .code_repository_status(summary.repository_id.clone())
        .await?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "sharded code repository status is missing after {stage}"
            ))
        })?;
    store
        .catalog
        .record_scope(summary.repository_id.clone(), summary.source_scope.clone())
        .await?;
    mirror_status(&store.control, status).await
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
