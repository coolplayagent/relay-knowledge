//! Resumes post-delta incremental finalization without rebuilding source facts.

use std::sync::Arc;

use crate::{
    api::ApiError,
    domain::{
        CodeIndexCheckpoint, CodeIndexMode, CodeIndexSession, CodeIndexSummary,
        code_incremental_clone, code_query_index_repair, code_reference_resolution,
        code_reference_resolution_query_index_repair, code_reference_search_query_index_repair,
        code_reference_search_rebuild, code_software_projection_phase,
    },
    storage::KnowledgeStore,
};

use super::task::{CodeIndexTaskLeaseContext, finalize_code_index_session_with_task_lease};

pub(super) enum IncrementalSnapshotApply {
    Complete(Box<CodeIndexSummary>),
    DurablePending {
        completed_steps: usize,
        max_steps: usize,
    },
    FinalizationRequired {
        checkpoint_state: String,
    },
    FullFallback,
}

pub(super) fn checkpoint_skips_parser(state: &str) -> bool {
    matches!(state, "finalizing:partitioned_publish" | "completed")
        || code_software_projection_phase(state).is_some()
        || code_query_index_repair(state).is_some()
        || code_reference_resolution(state).is_some()
        || code_reference_resolution_query_index_repair(state).is_some()
        || code_reference_search_query_index_repair(state).is_some()
        || code_reference_search_rebuild(state).is_some()
}

pub(super) fn should_resume_staged_full(
    mode: &CodeIndexMode,
    has_task_lease: bool,
    checkpoint_state: Option<&str>,
) -> bool {
    matches!(mode, CodeIndexMode::Incremental { .. })
        && has_task_lease
        && checkpoint_state.is_some_and(|state| code_incremental_clone(state).is_none())
}

pub(super) async fn resume_finalization(
    store: &Arc<dyn KnowledgeStore>,
    lease: Option<&CodeIndexTaskLeaseContext>,
    checkpoint: Option<&CodeIndexCheckpoint>,
) -> Result<Option<CodeIndexSummary>, ApiError> {
    let Some((lease, checkpoint, receipt)) =
        lease.zip(checkpoint).and_then(|(lease, checkpoint)| {
            checkpoint
                .incremental_summary
                .as_ref()
                .map(|receipt| (lease, checkpoint, receipt))
        })
    else {
        return Ok(None);
    };
    let content_identity_matches = checkpoint.repository_id
        == lease.publication_fence.repository_id
        && checkpoint.source_scope == lease.source_scope
        && checkpoint.tree_hash == lease.tree_hash
        && checkpoint.path_filters == lease.path_filters
        && checkpoint.language_filters == lease.language_filters;
    if !content_identity_matches {
        return Err(ApiError::internal(format!(
            "durable incremental finalization receipt for scope '{}' does not match its live task",
            checkpoint.source_scope
        )));
    }
    let terminal = matches!(
        checkpoint.state.as_str(),
        "completed" | "finalizing:partitioned_publish"
    );
    if receipt.task_id != lease.task_id {
        if terminal {
            // A later task may adopt the same terminal content-addressed scope. Its publication
            // transaction clears the previous task's receipt; until then the normal repair path
            // owns the adoption and must not consume another task's metrics.
            return Ok(None);
        }
        return Err(ApiError::internal(format!(
            "durable incremental finalization receipt for scope '{}' does not match its live task",
            checkpoint.source_scope
        )));
    }
    if checkpoint.resolved_commit_sha != lease.resolved_commit_sha
        || checkpoint.resource_budget != lease.resource_budget
    {
        return Err(ApiError::internal(format!(
            "durable incremental finalization receipt for scope '{}' does not match its live task",
            checkpoint.source_scope
        )));
    }
    if !(checkpoint.state == "indexing"
        || checkpoint.state.starts_with("finalizing:")
        || checkpoint.state == "completed")
    {
        return Err(ApiError::internal(format!(
            "durable incremental finalization receipt for scope '{}' does not match its live task",
            checkpoint.source_scope
        )));
    }
    let session = CodeIndexSession {
        repository_id: checkpoint.repository_id.clone(),
        source_scope: checkpoint.source_scope.clone(),
        base_resolved_commit_sha: Some(receipt.base_resolved_commit_sha.clone()),
        resolved_commit_sha: checkpoint.resolved_commit_sha.clone(),
        tree_hash: checkpoint.tree_hash.clone(),
        path_filters: checkpoint.path_filters.clone(),
        language_filters: checkpoint.language_filters.clone(),
        // The clone materialized a complete unpublished target. Full-replacement finalization
        // selects the paged fenced protocols; the receipt still reports the bounded delta work.
        full_replace: true,
        total_path_count: checkpoint.total_path_count,
        changed_path_count: receipt.changed_path_count,
        skipped_unchanged_count: receipt.skipped_unchanged_count,
        deleted_paths: Vec::new(),
        changed_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: checkpoint.resource_budget,
    };
    finalize_code_index_session_with_task_lease(store, lease, session)
        .await
        .map(Some)
}
