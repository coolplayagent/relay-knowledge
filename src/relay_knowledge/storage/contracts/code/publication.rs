use crate::domain::{
    CodeIndexBatch, CodeIndexCheckpoint, CodeIndexPublicationFence, CodeIndexSession,
    CodeIndexSnapshot, CodeIndexSummary,
};

use super::super::{StorageError, StorageFuture};
use super::CodeIndexFinalizationStep;

/// Checkpointed snapshot, batch, workspace, and final publication capability.
pub trait CodeIndexPublicationStore: Send + Sync {
    fn code_index_checkpoint(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>>;

    fn latest_code_index_checkpoint(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "latest code index checkpoint for repository '{repository_id}' is unavailable"
            )))
        })
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary>;

    fn apply_code_index_snapshot_with_fence(
        &self,
        snapshot: CodeIndexSnapshot,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped snapshot publication for task '{}' scope '{}' is unavailable",
                fence.task_id, snapshot.source_scope
            )))
        })
    }

    fn clear_code_workspace_state(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "workspace cleanup for repository '{repository_id}' scope '{source_scope}' is unavailable"
            )))
        })
    }

    /// Reports whether repository-owned auto-detected workspace artifacts
    /// still exist and therefore require a durable disabled-mode cleanup.
    fn code_repository_auto_workspace_state_exists(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, bool> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "auto workspace state inspection for repository '{repository_id}' is unavailable"
            )))
        })
    }

    fn clear_code_workspace_state_with_fence(
        &self,
        repository_id: String,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped workspace publication for task '{}' repository '{}' scope '{}' is unavailable",
                fence.task_id, repository_id, source_scope
            )))
        })
    }

    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "checkpointed code index sessions for scope '{}' are unavailable",
                session.source_scope
            )))
        })
    }

    fn begin_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped session startup for task '{}' scope '{}' is unavailable",
                fence.task_id, session.source_scope
            )))
        })
    }

    /// Starts a checkpointed session only when the durable checkpoint still
    /// matches the value observed during read-only plan validation.
    fn begin_code_index_session_at_checkpoint(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            let expectation = expected_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.source_scope.as_str())
                .unwrap_or("missing");
            Err(StorageError::InvalidInput(format!(
                "checkpoint-CAS session startup for scope '{}' at expectation '{}' is unavailable",
                session.source_scope, expectation
            )))
        })
    }

    /// Fenced variant of
    /// [`CodeIndexPublicationStore::begin_code_index_session_at_checkpoint`].
    fn begin_code_index_session_at_checkpoint_with_fence(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            let expectation = expected_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.source_scope.as_str())
                .unwrap_or("missing");
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped checkpoint-CAS session startup for task '{}' scope '{}' at expectation '{}' is unavailable",
                fence.task_id, session.source_scope, expectation
            )))
        })
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "checkpointed code index batches for scope '{}' are unavailable",
                batch.source_scope
            )))
        })
    }

    fn apply_code_index_batch_with_fence(
        &self,
        batch: CodeIndexBatch,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped batch publication for task '{}' scope '{}' is unavailable",
                fence.task_id, batch.source_scope
            )))
        })
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "checkpointed code index finalization for scope '{}' is unavailable",
                session.source_scope
            )))
        })
    }

    fn finalize_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped session finalization for task '{}' scope '{}' is unavailable",
                fence.task_id, session.source_scope
            )))
        })
    }

    fn advance_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexFinalizationStep> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped single-step finalization for task '{}' scope '{}' is unavailable",
                fence.task_id, session.source_scope
            )))
        })
    }
}
