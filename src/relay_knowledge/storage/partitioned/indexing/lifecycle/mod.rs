//! Snapshot and checkpointed-session publication into repository shards.

use std::sync::Arc;

use crate::{
    domain::{
        CodeIndexBatch, CodeIndexCheckpoint, CodeIndexPublicationFence, CodeIndexSession,
        CodeIndexSnapshot, CodeIndexSummary,
    },
    storage::{CodeRepositoryStore, StorageError, StorageFuture},
};

use super::super::{
    PartitionedSqliteKnowledgeStore,
    routing::current_control_scope,
    status::{mirror_status, mirror_status_with_fence},
};

pub(in crate::storage::partitioned) fn apply_snapshot(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: CodeIndexSnapshot,
) -> StorageFuture<'_, CodeIndexSummary> {
    apply_snapshot_inner(store, snapshot, None)
}

pub(in crate::storage::partitioned) fn apply_snapshot_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: CodeIndexSnapshot,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexSummary> {
    apply_snapshot_inner(store, snapshot, Some(fence))
}

fn apply_snapshot_inner(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: CodeIndexSnapshot,
    fence: Option<CodeIndexPublicationFence>,
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
        if let Some(fence) = fence.as_ref() {
            // ATTACH transactions spanning independent WAL files are not
            // power-loss atomic. Persist the semantic worktree rebind in the
            // control WAL first; the shard transaction then sees an exact,
            // idempotent target and only uses ATTACH for the attempt lock.
            store
                .catalog
                .prepare_snapshot_target(&snapshot, fence.clone())
                .await?;
        }
        let summary = match fence.clone() {
            Some(fence) => {
                shard
                    .apply_code_index_snapshot_with_fence(snapshot, fence)
                    .await?
            }
            None => shard.apply_code_index_snapshot(snapshot).await?,
        };
        publish_summary(&store, Arc::clone(&shard), &summary, "index", fence).await?;
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

pub(in crate::storage::partitioned) fn clear_workspace_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    repository_id: String,
    source_scope: String,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, ()> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .existing_repository_store(repository_id.clone())
            .await?
        {
            shard
                .clear_code_workspace_state_with_fence(
                    repository_id.clone(),
                    source_scope.clone(),
                    fence.clone(),
                )
                .await?;
        }
        store
            .control
            .clear_code_workspace_state_with_fence(repository_id, source_scope, fence)
            .await
    })
}

pub(in crate::storage::partitioned) fn begin_session(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    begin_session_inner(store, session, None)
}

pub(in crate::storage::partitioned) fn begin_session_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    begin_session_inner(store, session, Some(fence))
}

fn begin_session_inner(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    fence: Option<CodeIndexPublicationFence>,
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
        let checkpoint = match fence.clone() {
            Some(fence) => {
                shard
                    .begin_code_index_session_with_fence(session, fence)
                    .await?
            }
            None => shard.begin_code_index_session(session).await?,
        };
        match fence {
            Some(fence) => {
                store
                    .catalog
                    .stage_scope_with_fence(repository_id, source_scope, fence)
                    .await?
            }
            None => {
                store
                    .catalog
                    .stage_scope(repository_id, source_scope)
                    .await?
            }
        }
        Ok(checkpoint)
    })
}

pub(in crate::storage::partitioned) fn apply_batch(
    store: &PartitionedSqliteKnowledgeStore,
    batch: CodeIndexBatch,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    apply_batch_inner(store, batch, None)
}

pub(in crate::storage::partitioned) fn apply_batch_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    batch: CodeIndexBatch,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    apply_batch_inner(store, batch, Some(fence))
}

fn apply_batch_inner(
    store: &PartitionedSqliteKnowledgeStore,
    batch: CodeIndexBatch,
    fence: Option<CodeIndexPublicationFence>,
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
        let checkpoint = match fence.clone() {
            Some(fence) => {
                shard
                    .apply_code_index_batch_with_fence(batch, fence)
                    .await?
            }
            None => shard.apply_code_index_batch(batch).await?,
        };
        match fence {
            Some(fence) => {
                store
                    .catalog
                    .stage_scope_with_fence(repository_id, source_scope, fence)
                    .await?
            }
            None => {
                store
                    .catalog
                    .stage_scope(repository_id, source_scope)
                    .await?
            }
        }
        Ok(checkpoint)
    })
}

pub(in crate::storage::partitioned) fn finalize_session(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexSummary> {
    finalize_session_inner(store, session, None)
}

pub(in crate::storage::partitioned) fn finalize_session_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexSummary> {
    finalize_session_inner(store, session, Some(fence))
}

fn finalize_session_inner(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    fence: Option<CodeIndexPublicationFence>,
) -> StorageFuture<'_, CodeIndexSummary> {
    let store = store.clone();
    Box::pin(async move {
        let shard = store
            .catalog
            .staged_repository_store(session.repository_id.clone())
            .await?;
        let summary = match fence.clone() {
            Some(fence) => {
                shard
                    .finalize_code_index_session_with_fence(session, fence)
                    .await?
            }
            None => shard.finalize_code_index_session(session).await?,
        };
        publish_summary(&store, shard, &summary, "finalize", fence).await?;
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
    fence: Option<CodeIndexPublicationFence>,
) -> Result<(), StorageError> {
    let status = shard
        .code_repository_status(summary.repository_id.clone())
        .await?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "sharded code repository status is missing after {stage}"
            ))
        })?;
    match fence.clone() {
        Some(fence) => {
            store
                .catalog
                .record_scope_with_fence(
                    summary.repository_id.clone(),
                    summary.source_scope.clone(),
                    fence,
                )
                .await?
        }
        None => {
            store
                .catalog
                .record_scope(summary.repository_id.clone(), summary.source_scope.clone())
                .await?
        }
    }
    match fence {
        Some(fence) => mirror_status_with_fence(&store.control, status, fence).await,
        None => mirror_status(&store.control, status).await,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
