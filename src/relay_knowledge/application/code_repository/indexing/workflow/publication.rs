//! Durable snapshot publication, fallback, and finalization-fence stage.

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse},
    domain::{CodeIndexMode, CodeIndexSnapshot, CodeIndexSummary},
    storage::{CodeIndexPublicationTarget, StorageError},
};

use super::{
    super::{
        durable_incremental::{IncrementalSnapshotApply, resume_finalization},
        fast_path::published_task_response,
        task::{
            CodeIndexTaskLeaseContext, await_with_code_index_task_lease,
            code_index_task_lease_for_target,
        },
    },
    IndexWorkflowContext,
    snapshot::{self, GeneratedIndex},
};
use crate::application::code_repository::errors::storage_api_error;

pub(super) enum PublicationOutcome {
    Published(Box<CodeRepositoryIndexResponse>),
    Summary(Box<CodeIndexSummary>),
}

pub(super) async fn publish(
    workflow: &IndexWorkflowContext<'_>,
    generated: GeneratedIndex,
) -> Result<PublicationOutcome, ApiError> {
    let summary = match generated {
        GeneratedIndex::Recovered(summary) => summary,
        GeneratedIndex::Full(plan) => {
            workflow
                .service
                .apply_code_index_from_plan(&workflow.store, plan, workflow.task_lease.clone())
                .await?
        }
        GeneratedIndex::Incremental(snapshot) => {
            if let Some(response) = reconcile_incremental_target(workflow, &snapshot).await? {
                return Ok(PublicationOutcome::Published(Box::new(response)));
            }
            match publish_incremental(workflow, snapshot).await? {
                Some(summary) => summary,
                None => {
                    let plan = snapshot::prepare_full_plan(workflow).await?;
                    workflow
                        .service
                        .apply_code_index_from_plan(
                            &workflow.store,
                            plan,
                            workflow.task_lease.clone(),
                        )
                        .await?
                }
            }
        }
    };

    reconcile_finalization(workflow, summary).await
}

async fn reconcile_incremental_target(
    workflow: &IndexWorkflowContext<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<Option<CodeRepositoryIndexResponse>, ApiError> {
    let Some(lease) = workflow.task_lease.as_ref() else {
        return Ok(None);
    };
    let actual_target = CodeIndexPublicationTarget {
        task_id: lease.task_id.clone(),
        repository_id: snapshot.repository_id.clone(),
        source_scope: snapshot.source_scope.clone(),
        resolved_commit_sha: snapshot.resolved_commit_sha.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        path_filters: snapshot.path_filters.clone(),
        language_filters: snapshot.language_filters.clone(),
    };
    if !workflow
        .store
        .reconcile_code_index_publication_with_fence(
            actual_target.clone(),
            lease.publication_fence.clone(),
        )
        .await
        .map_err(storage_api_error)?
    {
        return Ok(None);
    }
    let actual_lease = code_index_task_lease_for_target(
        lease,
        &actual_target.repository_id,
        actual_target.source_scope,
        actual_target.resolved_commit_sha,
        actual_target.tree_hash,
    )?;
    published_task_response(
        &workflow.store,
        &workflow.status,
        &workflow.request,
        &workflow.context,
        &actual_lease,
    )
    .await
    .map(Some)
}

async fn publish_incremental(
    workflow: &IndexWorkflowContext<'_>,
    snapshot: CodeIndexSnapshot,
) -> Result<Option<CodeIndexSummary>, ApiError> {
    let durable_fallback_allowed = incremental_snapshot_matches_lease(
        &workflow.request.mode,
        &snapshot,
        workflow.task_lease.as_ref(),
    );
    let mut previous_clone_step = None;
    let mut clone_attempt_count = 0usize;
    loop {
        let attempt_snapshot = snapshot.clone();
        let advance = await_with_code_index_task_lease(
            &workflow.store,
            workflow.task_lease.as_ref(),
            async {
                let result = match workflow.task_lease.as_ref() {
                    Some(lease) => {
                        workflow
                            .store
                            .apply_code_index_snapshot_with_fence(
                                attempt_snapshot,
                                lease.publication_fence.clone(),
                            )
                            .await
                    }
                    None => {
                        workflow
                            .store
                            .apply_code_index_snapshot(attempt_snapshot)
                            .await
                    }
                };
                match result {
                    Ok(summary) => Ok(IncrementalSnapshotApply::Complete(Box::new(summary))),
                    Err(StorageError::DurableStagingPending {
                        completed_steps,
                        max_steps,
                    }) => Ok(IncrementalSnapshotApply::DurablePending {
                        completed_steps,
                        max_steps,
                    }),
                    Err(StorageError::DurableFinalizationRequired { checkpoint_state }) => {
                        Ok(IncrementalSnapshotApply::FinalizationRequired { checkpoint_state })
                    }
                    Err(StorageError::DurableStagingRequired(_)) if durable_fallback_allowed => {
                        Ok(IncrementalSnapshotApply::FullFallback)
                    }
                    Err(error) => Err(storage_api_error(error)),
                }
            },
        )
        .await?;
        match advance {
            IncrementalSnapshotApply::Complete(summary) => return Ok(Some(*summary)),
            IncrementalSnapshotApply::FullFallback => return Ok(None),
            IncrementalSnapshotApply::FinalizationRequired { checkpoint_state } => {
                let checkpoint = workflow
                    .store
                    .code_index_checkpoint(snapshot.source_scope.clone())
                    .await
                    .map_err(storage_api_error)?;
                if checkpoint
                    .as_ref()
                    .is_none_or(|checkpoint| checkpoint.state != checkpoint_state)
                {
                    return Err(ApiError::internal(format!(
                        "durable incremental finalization handoff for scope '{}' was not committed atomically",
                        snapshot.source_scope
                    )));
                }
                let actual_lease = workflow
                    .task_lease
                    .as_ref()
                    .map(|lease| {
                        code_index_task_lease_for_target(
                            lease,
                            &snapshot.repository_id,
                            snapshot.source_scope.clone(),
                            snapshot.resolved_commit_sha.clone(),
                            snapshot.tree_hash.clone(),
                        )
                    })
                    .transpose()?;
                let summary = resume_finalization(
                    &workflow.store,
                    actual_lease.as_ref(),
                    checkpoint.as_ref(),
                )
                .await?
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "durable incremental finalization receipt for scope '{}' is missing",
                        snapshot.source_scope
                    ))
                })?;
                return Ok(Some(summary));
            }
            IncrementalSnapshotApply::DurablePending {
                completed_steps,
                max_steps,
            } => {
                clone_attempt_count = clone_attempt_count.saturating_add(1);
                let progressed =
                    previous_clone_step.is_none_or(|previous| completed_steps > previous);
                if max_steps == 0
                    || completed_steps > max_steps
                    || clone_attempt_count > max_steps.saturating_add(1)
                    || !progressed
                {
                    return Err(ApiError::internal(format!(
                        "durable incremental clone for scope '{}' did not advance within its step proof",
                        snapshot.source_scope
                    )));
                }
                previous_clone_step = Some(completed_steps);
            }
        }
    }
}

async fn reconcile_finalization(
    workflow: &IndexWorkflowContext<'_>,
    summary: CodeIndexSummary,
) -> Result<PublicationOutcome, ApiError> {
    let Some(lease) = workflow.task_lease.as_ref() else {
        return Ok(PublicationOutcome::Summary(Box::new(summary)));
    };
    let actual_lease = code_index_task_lease_for_target(
        lease,
        &summary.repository_id,
        summary.source_scope.clone(),
        summary.resolved_commit_sha.clone(),
        summary.tree_hash.clone(),
    )?;
    let checkpoint = workflow
        .store
        .code_index_checkpoint(summary.source_scope.clone())
        .await
        .map_err(storage_api_error)?;
    let Some(checkpoint) = checkpoint.as_ref().filter(|checkpoint| {
        matches!(
            checkpoint.state.as_str(),
            "finalizing:partitioned_publish" | "completed"
        )
    }) else {
        return Ok(PublicationOutcome::Summary(Box::new(summary)));
    };
    let reconciled = workflow
        .store
        .reconcile_code_index_publication_with_fence(
            CodeIndexPublicationTarget {
                task_id: actual_lease.task_id.clone(),
                repository_id: summary.repository_id.clone(),
                source_scope: summary.source_scope.clone(),
                resolved_commit_sha: summary.resolved_commit_sha.clone(),
                tree_hash: summary.tree_hash.clone(),
                path_filters: actual_lease.path_filters.clone(),
                language_filters: actual_lease.language_filters.clone(),
            },
            actual_lease.publication_fence.clone(),
        )
        .await
        .map_err(storage_api_error)?;
    if reconciled {
        return published_task_response(
            &workflow.store,
            &workflow.status,
            &workflow.request,
            &workflow.context,
            &actual_lease,
        )
        .await
        .map(|response| PublicationOutcome::Published(Box::new(response)));
    }
    if checkpoint.state == "finalizing:partitioned_publish" {
        return Err(ApiError::storage_unavailable(format!(
            "partitioned finalization for scope '{}' did not publish its catalog handoff",
            summary.source_scope
        )));
    }
    Ok(PublicationOutcome::Summary(Box::new(summary)))
}

pub(in crate::application::code_repository::indexing) fn incremental_snapshot_matches_lease(
    mode: &CodeIndexMode,
    snapshot: &CodeIndexSnapshot,
    lease: Option<&CodeIndexTaskLeaseContext>,
) -> bool {
    let Some(lease) = lease.filter(|_| matches!(mode, CodeIndexMode::Incremental { .. })) else {
        return false;
    };
    snapshot.repository_id == lease.publication_fence.repository_id
        && snapshot.source_scope == lease.source_scope
        && snapshot.resolved_commit_sha == lease.resolved_commit_sha
        && snapshot.tree_hash == lease.tree_hash
        && snapshot.path_filters == lease.path_filters
        && snapshot.language_filters == lease.language_filters
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
