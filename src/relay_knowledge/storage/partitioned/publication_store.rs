//! Routes fenced code-index publication through the repository shard lifecycle.

use super::{PartitionedSqliteKnowledgeStore, indexing};
use crate::{
    domain::{
        CodeIndexBatch, CodeIndexCheckpoint, CodeIndexPublicationFence, CodeIndexSession,
        CodeIndexSnapshot, CodeIndexSummary,
    },
    storage::{CodeIndexPublicationStore, StorageFuture},
};

impl CodeIndexPublicationStore for PartitionedSqliteKnowledgeStore {
    fn cleanup_source_replan_with_fence(
        &self,
        source_scope: String,
        fence: CodeIndexPublicationFence,
        resume_only: bool,
    ) -> StorageFuture<'_, bool> {
        indexing::lifecycle::cleanup_source_replan(self, source_scope, fence, resume_only)
    }

    fn code_index_checkpoint(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
        indexing::checkpoint::by_scope(self, source_scope)
    }

    fn latest_code_index_checkpoint(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
        indexing::checkpoint::latest(self, repository_id)
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        indexing::lifecycle::apply_snapshot(self, snapshot)
    }

    fn apply_code_index_snapshot_with_fence(
        &self,
        snapshot: CodeIndexSnapshot,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        indexing::lifecycle::apply_snapshot_with_fence(self, snapshot, fence)
    }

    fn clear_code_workspace_state(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> StorageFuture<'_, ()> {
        indexing::lifecycle::clear_workspace(self, repository_id, source_scope)
    }

    fn code_repository_auto_workspace_state_exists(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, bool> {
        indexing::lifecycle::auto_workspace_state_exists(self, repository_id)
    }

    fn clear_code_workspace_state_with_fence(
        &self,
        repository_id: String,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, ()> {
        indexing::lifecycle::clear_workspace_with_fence(self, repository_id, source_scope, fence)
    }
    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::begin_session(self, session)
    }

    fn begin_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::begin_session_with_fence(self, session, fence)
    }

    fn begin_code_index_session_at_checkpoint(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::begin_session_at_checkpoint(self, session, expected_checkpoint)
    }

    fn begin_code_index_session_at_checkpoint_with_fence(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::begin_session_at_checkpoint_with_fence(
            self,
            session,
            expected_checkpoint,
            fence,
        )
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::apply_batch(self, batch)
    }

    fn apply_code_index_batch_with_fence(
        &self,
        batch: CodeIndexBatch,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::apply_batch_with_fence(self, batch, fence)
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        indexing::lifecycle::finalize_session(self, session)
    }

    fn finalize_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        indexing::lifecycle::finalize_session_with_fence(self, session, fence)
    }

    fn advance_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, crate::storage::CodeIndexFinalizationStep> {
        indexing::lifecycle::advance_session_with_fence(self, session, fence)
    }
}
