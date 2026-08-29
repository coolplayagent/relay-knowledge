//! Checkpoint recovery and publication-fence reconciliation stage.

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse},
    domain::{CodeIndexCheckpoint, CodeIndexMode, CodeIndexSummary},
    storage::CodeIndexPublicationTarget,
};

use super::{
    super::{
        durable_incremental::{resume_finalization, should_resume_staged_full},
        fast_path::published_task_response,
    },
    IndexWorkflowContext,
};
use crate::application::code_repository::errors::storage_api_error;

pub(super) enum RecoveryOutcome {
    Published(Box<CodeRepositoryIndexResponse>),
    Continue(Box<RecoveryState>),
}

pub(super) struct RecoveryState {
    pub(super) resume_staged_full: bool,
    pub(super) resumed_summary: Option<CodeIndexSummary>,
}

pub(super) async fn recover_and_reconcile(
    workflow: &IndexWorkflowContext<'_>,
) -> Result<RecoveryOutcome, ApiError> {
    if let Some(lease) = workflow.task_lease.as_ref() {
        let reconciled = workflow
            .store
            .reconcile_code_index_publication_with_fence(
                CodeIndexPublicationTarget {
                    task_id: lease.task_id.clone(),
                    repository_id: lease.publication_fence.repository_id.clone(),
                    source_scope: lease.source_scope.clone(),
                    resolved_commit_sha: lease.resolved_commit_sha.clone(),
                    tree_hash: lease.tree_hash.clone(),
                    path_filters: lease.path_filters.clone(),
                    language_filters: lease.language_filters.clone(),
                },
                lease.publication_fence.clone(),
            )
            .await
            .map_err(storage_api_error)?;
        if reconciled {
            let response = published_task_response(
                &workflow.store,
                &workflow.status,
                &workflow.request,
                &workflow.context,
                lease,
            )
            .await?;
            return Ok(RecoveryOutcome::Published(Box::new(response)));
        }
    }

    let staged_checkpoint = staged_checkpoint(workflow).await?;
    let resume_staged_full = should_resume_staged_full(
        &workflow.request.mode,
        workflow.task_lease.is_some(),
        staged_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.state.as_str()),
    );
    let resumed_summary = resume_finalization(
        &workflow.store,
        workflow.task_lease.as_ref(),
        staged_checkpoint.as_ref(),
    )
    .await?;

    Ok(RecoveryOutcome::Continue(Box::new(RecoveryState {
        resume_staged_full,
        resumed_summary,
    })))
}

async fn staged_checkpoint(
    workflow: &IndexWorkflowContext<'_>,
) -> Result<Option<CodeIndexCheckpoint>, ApiError> {
    match workflow.task_lease.as_ref() {
        Some(lease) if matches!(&workflow.request.mode, CodeIndexMode::Incremental { .. }) => {
            workflow
                .store
                .code_index_checkpoint(lease.source_scope.clone())
                .await
                .map_err(storage_api_error)
        }
        _ => Ok(None),
    }
}
