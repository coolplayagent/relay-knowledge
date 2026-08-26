//! Snapshot and checkpointed-session publication into repository shards.

use std::sync::Arc;

use crate::{
    domain::{
        CodeIndexBatch, CodeIndexCheckpoint, CodeIndexPublicationFence, CodeIndexSession,
        CodeIndexSnapshot, CodeIndexSummary,
    },
    storage::{CodeRepositoryStore, StorageError, StorageFuture},
};

#[cfg(test)]
use crate::storage::BusinessKnowledgeStore;

use super::super::{PartitionedSqliteKnowledgeStore, status::mirror_status};

pub(in crate::storage::partitioned) fn apply_snapshot(
    _store: &PartitionedSqliteKnowledgeStore,
    snapshot: CodeIndexSnapshot,
) -> StorageFuture<'_, CodeIndexSummary> {
    reject_unfenced_mutation("snapshot", snapshot.repository_id)
}

pub(in crate::storage::partitioned) fn apply_snapshot_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: CodeIndexSnapshot,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexSummary> {
    apply_snapshot_inner(store, snapshot, Some(fence))
}

/// Seeds partitioned fixtures without changing the production trait contract.
#[cfg(test)]
pub(in crate::storage::partitioned) fn seed_snapshot_for_test(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: CodeIndexSnapshot,
) -> StorageFuture<'_, CodeIndexSummary> {
    let store = store.clone();
    Box::pin(async move {
        let projection = crate::domain::BusinessKnowledgeProjectionInput {
            repository_id: snapshot.repository_id.clone(),
            source_scope: snapshot.source_scope.clone(),
            resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
            sources: Vec::new(),
        };
        let summary = apply_snapshot_inner(&store, snapshot, None).await?;
        store
            .replace_business_knowledge_projection(projection)
            .await?;
        store
            .refresh_software_global_projection(summary.source_scope.clone())
            .await?;
        Ok(summary)
    })
}

fn apply_snapshot_inner(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: CodeIndexSnapshot,
    fence: Option<CodeIndexPublicationFence>,
) -> StorageFuture<'_, CodeIndexSummary> {
    let store = store.clone();
    Box::pin(async move {
        let repository_id = snapshot.repository_id.clone();
        let source_scope = snapshot.source_scope.clone();
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
            .import_control_repository_metadata(Arc::clone(&shard), snapshot.repository_id.clone())
            .await?;
        if let Some(base_scope) = base_scope.as_deref() {
            require_incremental_base_in_shard(&shard, &snapshot, base_scope).await?;
        }
        if let Some(fence) = fence.as_ref() {
            // ATTACH transactions spanning independent WAL files are not
            // power-loss atomic. Persist the semantic worktree rebind in the
            // control WAL first; the shard transaction then sees an exact,
            // idempotent target and only uses ATTACH for the attempt lock.
            store
                .catalog
                .prepare_snapshot_target(&snapshot, fence.clone())
                .await?;
            // The catalog route is the crash-recovery locator for every shard-side clone page and
            // delta handoff. Persist it before the first shard mutation so a lost response can
            // resume the same task/fence instead of orphaning an otherwise durable checkpoint.
            store
                .catalog
                .stage_scope_with_fence(repository_id.clone(), source_scope.clone(), fence.clone())
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
        if fence.is_none() {
            publish_summary(&store, Arc::clone(&shard), &summary, "index").await?;
        }
        Ok(summary)
    })
}

pub(in crate::storage::partitioned) fn clear_workspace(
    _store: &PartitionedSqliteKnowledgeStore,
    repository_id: String,
    _source_scope: String,
) -> StorageFuture<'_, ()> {
    reject_unfenced_mutation("workspace cleanup", repository_id)
}

/// Clears workspace fixture state without changing the fenced product API.
#[cfg(test)]
pub(in crate::storage::partitioned) fn seed_clear_workspace_for_test(
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

pub(in crate::storage::partitioned) fn auto_workspace_state_exists(
    store: &PartitionedSqliteKnowledgeStore,
    repository_id: String,
) -> StorageFuture<'_, bool> {
    let store = store.clone();
    Box::pin(async move {
        if store
            .control
            .code_repository_auto_workspace_state_exists(repository_id.clone())
            .await?
        {
            return Ok(true);
        }
        let Some(shard) = store
            .catalog
            .existing_repository_store(repository_id.clone())
            .await?
        else {
            return Ok(false);
        };
        shard
            .code_repository_auto_workspace_state_exists(repository_id)
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
    _store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    reject_unfenced_mutation("session start", session.repository_id)
}

pub(in crate::storage::partitioned) fn begin_session_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    begin_session_inner(
        store,
        session,
        CheckpointExpectation::Unchecked,
        Some(fence),
    )
}

/// Seeds checkpoint fixtures without exposing an unfenced production entry.
#[cfg(test)]
pub(in crate::storage::partitioned) fn seed_session_for_test(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    begin_session_inner(store, session, CheckpointExpectation::Unchecked, None)
}

/// Seeds an exact checkpoint-resume assertion without exposing an unfenced
/// product entry point.
#[cfg(test)]
pub(in crate::storage::partitioned) fn seed_session_at_checkpoint_for_test(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    expected_checkpoint: Option<CodeIndexCheckpoint>,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    begin_session_inner(
        store,
        session,
        CheckpointExpectation::Exact(Box::new(expected_checkpoint)),
        None,
    )
}

pub(in crate::storage::partitioned) fn begin_session_at_checkpoint(
    _store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    _expected_checkpoint: Option<CodeIndexCheckpoint>,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    reject_unfenced_mutation("session resume", session.repository_id)
}

pub(in crate::storage::partitioned) fn begin_session_at_checkpoint_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    expected_checkpoint: Option<CodeIndexCheckpoint>,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    begin_session_inner(
        store,
        session,
        CheckpointExpectation::Exact(Box::new(expected_checkpoint)),
        Some(fence),
    )
}

enum CheckpointExpectation {
    Unchecked,
    Exact(Box<Option<CodeIndexCheckpoint>>),
}

fn begin_session_inner(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    checkpoint_expectation: CheckpointExpectation,
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
        let raw_expectation = match checkpoint_expectation {
            CheckpointExpectation::Unchecked => None,
            CheckpointExpectation::Exact(expected) => Some(
                validated_raw_checkpoint_expectation(
                    &store,
                    Arc::clone(&shard),
                    &source_scope,
                    *expected,
                    fence.clone(),
                )
                .await?,
            ),
        };
        store
            .catalog
            .import_control_repository_metadata(Arc::clone(&shard), repository_id.clone())
            .await?;
        match fence.as_ref() {
            Some(fence) => {
                store
                    .catalog
                    .stage_scope_with_fence(
                        repository_id.clone(),
                        source_scope.clone(),
                        fence.clone(),
                    )
                    .await?;
            }
            None => {
                store
                    .catalog
                    .stage_scope(repository_id.clone(), source_scope.clone())
                    .await?;
            }
        }
        let checkpoint = match (fence.clone(), raw_expectation) {
            (Some(fence), Some(expected)) => {
                shard
                    .begin_code_index_session_at_checkpoint_with_fence(session, expected, fence)
                    .await?
            }
            (Some(fence), None) => {
                shard
                    .begin_code_index_session_with_fence(session, fence)
                    .await?
            }
            (None, Some(expected)) => {
                shard
                    .begin_code_index_session_at_checkpoint(session, expected)
                    .await?
            }
            (None, None) => shard.begin_code_index_session(session).await?,
        };
        Ok(checkpoint)
    })
}

async fn validated_raw_checkpoint_expectation(
    store: &PartitionedSqliteKnowledgeStore,
    shard: Arc<crate::storage::SqliteGraphStore>,
    source_scope: &str,
    expected_checkpoint: Option<CodeIndexCheckpoint>,
    fence: Option<CodeIndexPublicationFence>,
) -> Result<Option<CodeIndexCheckpoint>, StorageError> {
    let raw_checkpoint = shard.code_index_checkpoint(source_scope.to_owned()).await?;
    let mut projected_checkpoint = raw_checkpoint.clone();
    let mut active = false;
    let mut query_indexes_ready = true;
    if let Some(checkpoint) = projected_checkpoint.as_mut() {
        active = store
            .catalog
            .active_repository_for_scope(source_scope.to_owned())
            .await?
            .as_deref()
            == Some(checkpoint.repository_id.as_str());
        query_indexes_ready = shard.code_query_indexes_ready_for_publication().await?;
        super::checkpoint::project_publication_state(checkpoint, active, query_indexes_ready);
    }
    if projected_checkpoint != expected_checkpoint {
        return Err(StorageError::Invariant(format!(
            "partitioned checkpoint for scope '{source_scope}' changed after read-only plan validation"
        )));
    }
    let Some(raw) = raw_checkpoint else {
        return Ok(None);
    };
    if raw.state == "completed" && !active && !query_indexes_ready {
        let fence = fence.ok_or_else(|| {
            StorageError::Invariant(format!(
                "inactive completed partitioned checkpoint for scope '{source_scope}' requires a current publication fence before repair"
            ))
        })?;
        if !store
            .catalog
            .staged_scope_owned_by_task(
                raw.repository_id.clone(),
                source_scope.to_owned(),
                fence.task_id.clone(),
            )
            .await?
        {
            return Err(StorageError::Invariant(format!(
                "inactive completed partitioned checkpoint for scope '{source_scope}' is not staged by its fenced task"
            )));
        }
        return shard
            .reopen_completed_checkpoint_for_partitioned_repair(raw, fence)
            .await
            .map(Some);
    }
    if raw.state != "finalizing:partitioned_publish" || !active || !query_indexes_ready {
        return Ok(Some(raw));
    }
    let completed = shard
        .materialize_partitioned_completed_checkpoint(raw, fence)
        .await?;
    let remains_active = store
        .catalog
        .active_repository_for_scope(source_scope.to_owned())
        .await?
        .as_deref()
        == Some(completed.repository_id.as_str());
    if !remains_active {
        return Err(StorageError::Invariant(format!(
            "partitioned scope '{source_scope}' changed publication state while its checkpoint was materialized"
        )));
    }

    Ok(Some(completed))
}

pub(in crate::storage::partitioned) fn apply_batch(
    _store: &PartitionedSqliteKnowledgeStore,
    batch: CodeIndexBatch,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    reject_unfenced_mutation("batch publication", batch.repository_id)
}

pub(in crate::storage::partitioned) fn apply_batch_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    batch: CodeIndexBatch,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    apply_batch_inner(store, batch, Some(fence))
}

/// Seeds batch fixtures without exposing an unfenced production entry.
#[cfg(test)]
pub(in crate::storage::partitioned) fn seed_batch_for_test(
    store: &PartitionedSqliteKnowledgeStore,
    batch: CodeIndexBatch,
) -> StorageFuture<'_, CodeIndexCheckpoint> {
    apply_batch_inner(store, batch, None)
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
        store
            .catalog
            .import_control_repository_metadata(Arc::clone(&shard), repository_id.clone())
            .await?;
        match fence.as_ref() {
            Some(fence) => {
                store
                    .catalog
                    .stage_scope_with_fence(
                        repository_id.clone(),
                        source_scope.clone(),
                        fence.clone(),
                    )
                    .await?;
            }
            None => {
                store
                    .catalog
                    .stage_scope(repository_id.clone(), source_scope.clone())
                    .await?;
            }
        }
        let checkpoint = match fence.clone() {
            Some(fence) => {
                shard
                    .apply_code_index_batch_with_fence(batch, fence)
                    .await?
            }
            None => shard.apply_code_index_batch(batch).await?,
        };
        Ok(checkpoint)
    })
}

pub(in crate::storage::partitioned) fn finalize_session(
    _store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexSummary> {
    reject_unfenced_mutation("session finalization", session.repository_id)
}

pub(in crate::storage::partitioned) fn finalize_session_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, CodeIndexSummary> {
    finalize_session_inner(store, session, Some(fence))
}

/// Finalizes legacy fixture setup without exposing an unfenced product API.
#[cfg(test)]
pub(in crate::storage::partitioned) fn seed_finalize_session_for_test(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
) -> StorageFuture<'_, CodeIndexSummary> {
    finalize_session_inner(store, session, None)
}

pub(in crate::storage::partitioned) fn advance_session_with_fence(
    store: &PartitionedSqliteKnowledgeStore,
    session: CodeIndexSession,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, crate::storage::CodeIndexFinalizationStep> {
    let store = store.clone();
    Box::pin(async move {
        let shard = store
            .catalog
            .staged_repository_store(session.repository_id.clone())
            .await?;
        shard
            .advance_code_index_session_with_fence(session, fence)
            .await
    })
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
        if fence.is_none() {
            publish_summary(&store, shard, &summary, "finalize").await?;
        }
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
    let base_commit = snapshot.base_resolved_commit_sha.clone().ok_or_else(|| {
        durable_staging_required(
            &snapshot.repository_id,
            "the incremental snapshot has no resolved base commit",
        )
    })?;
    let base_scope = store
        .control
        .code_repository_scope_status(
            snapshot.repository_id.clone(),
            base_commit,
            snapshot.path_filters.clone(),
            snapshot.language_filters.clone(),
        )
        .await?
        .and_then(|status| status.last_indexed_scope_id)
        .ok_or_else(|| {
            durable_staging_required(
                &snapshot.repository_id,
                "the control plane has no matching incremental base scope",
            )
        })?;

    Ok(Some(base_scope))
}

async fn require_incremental_base_in_shard(
    shard: &Arc<crate::storage::SqliteGraphStore>,
    snapshot: &CodeIndexSnapshot,
    base_scope: &str,
) -> Result<(), StorageError> {
    let base_commit = snapshot.base_resolved_commit_sha.clone().ok_or_else(|| {
        durable_staging_required(
            &snapshot.repository_id,
            "the incremental snapshot has no resolved base commit",
        )
    })?;
    let local_base = shard
        .code_repository_scope_status(
            snapshot.repository_id.clone(),
            base_commit,
            snapshot.path_filters.clone(),
            snapshot.language_filters.clone(),
        )
        .await?;
    if local_base.as_ref().is_none_or(|status| {
        status.stale || status.last_indexed_scope_id.as_deref() != Some(base_scope)
    }) {
        return Err(durable_staging_required(
            &snapshot.repository_id,
            "the repository shard does not contain the metadata-selected incremental base scope",
        ));
    }
    Ok(())
}

fn durable_staging_required(repository_id: &str, reason: &str) -> StorageError {
    StorageError::DurableStagingRequired(format!(
        "partitioned incremental publication for repository '{repository_id}' requires the checkpointed full-index pipeline because {reason}"
    ))
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

fn reject_unfenced_mutation<T>(
    operation: &'static str,
    repository_id: String,
) -> StorageFuture<'static, T>
where
    T: Send + 'static,
{
    Box::pin(async move {
        Err(StorageError::InvalidInput(format!(
            "partitioned_sqlite {operation} for repository '{repository_id}' requires a durable code-index task publication fence; queue and claim the task, then use the fenced storage operation"
        )))
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "publication_barrier_tests.rs"]
mod publication_barrier_tests;

#[cfg(test)]
#[path = "query_index_repair_tests.rs"]
mod query_index_repair_tests;

#[cfg(test)]
#[path = "reference_search_page_tests.rs"]
mod reference_search_page_tests;

#[cfg(test)]
#[path = "unfenced_authority_tests.rs"]
mod unfenced_authority_tests;
