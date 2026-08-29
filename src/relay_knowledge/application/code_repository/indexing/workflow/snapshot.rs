//! Full-plan and incremental-snapshot generation stage.

use crate::{
    api::ApiError,
    code::{
        CodeIndexPlan, build_index_snapshot_with_workspace_detection,
        prepare_full_index_plan_with_workspace_detection,
    },
    domain::{CodeIndexMode, CodeIndexSnapshot, CodeIndexSummary},
};

use super::{
    super::{
        super::blocking::run_blocking_code, state::previous_index_state_for_index,
        task::await_with_code_index_task_lease,
    },
    IndexWorkflowContext,
    recovery::RecoveryState,
};

pub(super) enum GeneratedIndex {
    Recovered(CodeIndexSummary),
    Full(CodeIndexPlan),
    Incremental(CodeIndexSnapshot),
}

pub(super) async fn generate(
    workflow: &IndexWorkflowContext<'_>,
    recovery: RecoveryState,
) -> Result<GeneratedIndex, ApiError> {
    if let Some(summary) = recovery.resumed_summary {
        return Ok(GeneratedIndex::Recovered(summary));
    }
    if workflow.request.mode == CodeIndexMode::Full || recovery.resume_staged_full {
        return prepare_full_plan(workflow).await.map(GeneratedIndex::Full);
    }

    let previous =
        previous_index_state_for_index(&workflow.store, &workflow.status, &workflow.request)
            .await?;
    let mode = workflow.request.mode.clone();
    let workspace_detection = workflow.request.workspace_detection.clone();
    let registration = workflow.registration.clone();
    let selector = workflow.request.repository.clone();
    let snapshot = await_with_code_index_task_lease(
        &workflow.store,
        workflow.task_lease.as_ref(),
        run_blocking_code(move || {
            build_index_snapshot_with_workspace_detection(
                &registration,
                &selector,
                mode,
                previous.fingerprints,
                previous.base_resolved_commit_sha,
                &workspace_detection,
            )
        }),
    )
    .await?;
    Ok(GeneratedIndex::Incremental(snapshot))
}

pub(super) async fn prepare_full_plan(
    workflow: &IndexWorkflowContext<'_>,
) -> Result<CodeIndexPlan, ApiError> {
    let registration = workflow.registration.clone();
    let selector = workflow.request.repository.clone();
    let workspace_detection = workflow.request.workspace_detection.clone();
    let resource_budget = workflow
        .task_lease
        .as_ref()
        .map(|lease| lease.resource_budget)
        .unwrap_or_default();
    await_with_code_index_task_lease(
        &workflow.store,
        workflow.task_lease.as_ref(),
        run_blocking_code(move || {
            prepare_full_index_plan_with_workspace_detection(
                registration,
                selector,
                resource_budget,
                &workspace_detection,
            )
        }),
    )
    .await
}
