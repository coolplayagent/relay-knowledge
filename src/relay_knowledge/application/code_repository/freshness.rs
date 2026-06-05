use std::{collections::BTreeSet, sync::Arc};

use crate::{
    api::{
        ApiError, CodeRepositoryFreshnessCursor, CodeRepositoryFreshnessDiagnostics,
        CodeRepositoryFreshnessInput, CodeRepositoryPendingIndexWork,
    },
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeRepositorySelector, CodeRepositoryStatus,
        CodeRetrievalHit, CodeRetrievalRequest,
    },
    storage::KnowledgeStore,
};

use super::support::{
    active_index_matches_request, code_status_checkpoint, indexed_source_scope, storage_api_error,
};

pub(super) struct CodeQueryFreshnessContext<'a> {
    pub(super) base_status: &'a CodeRepositoryStatus,
    pub(super) scoped_status: &'a CodeRepositoryStatus,
    pub(super) request: &'a CodeRetrievalRequest,
    pub(super) requested_ref: String,
    pub(super) requested_resolved_ref: String,
    pub(super) freshness_target: CodeRepositorySelector,
    pub(super) stale_reason: Option<String>,
    pub(super) degraded_reason: Option<String>,
    pub(super) results: &'a [CodeRetrievalHit],
    pub(super) graph_version: u64,
}

pub(super) struct CodeFeatureFlagFreshnessContext<'a> {
    pub(super) base_status: &'a CodeRepositoryStatus,
    pub(super) scoped_status: &'a CodeRepositoryStatus,
    pub(super) request: &'a CodeFeatureFlagRequest,
    pub(super) requested_ref: String,
    pub(super) requested_resolved_ref: String,
    pub(super) freshness_target: CodeRepositorySelector,
    pub(super) stale_reason: Option<String>,
    pub(super) degraded_reason: Option<String>,
    pub(super) flags: &'a [CodeFeatureFlagGraph],
    pub(super) graph_version: u64,
}

pub(super) async fn code_query_freshness_diagnostics(
    store: &Arc<dyn KnowledgeStore>,
    context: CodeQueryFreshnessContext<'_>,
) -> Result<CodeRepositoryFreshnessDiagnostics, ApiError> {
    let (pending, checkpoint) = pending_work_and_checkpoint(
        store,
        context.base_status,
        context.scoped_status,
        &context.freshness_target,
    )
    .await?;
    let source_scope = indexed_source_scope(context.scoped_status);

    Ok(CodeRepositoryFreshnessDiagnostics::code_query(
        CodeRepositoryFreshnessInput {
            graph_version: context.graph_version,
            freshness_policy: context.request.freshness_policy,
            source_scope,
            requested_ref: context.requested_ref,
            requested_resolved_ref: context.requested_resolved_ref,
            served_ref: context.request.repository.ref_selector.clone(),
            scope_stale: context.scoped_status.stale || context.stale_reason.is_some(),
            stale_reason: context.stale_reason,
            degraded_reason: context.degraded_reason,
            pending,
            cursor: checkpoint,
            direct_source_read_paths: result_paths(context.results),
        },
    ))
}

pub(super) async fn code_feature_flag_freshness_diagnostics(
    store: &Arc<dyn KnowledgeStore>,
    context: CodeFeatureFlagFreshnessContext<'_>,
) -> Result<CodeRepositoryFreshnessDiagnostics, ApiError> {
    let (pending, checkpoint) = pending_work_and_checkpoint(
        store,
        context.base_status,
        context.scoped_status,
        &context.freshness_target,
    )
    .await?;
    let source_scope = indexed_source_scope(context.scoped_status);

    Ok(CodeRepositoryFreshnessDiagnostics::code_query(
        CodeRepositoryFreshnessInput {
            graph_version: context.graph_version,
            freshness_policy: context.request.freshness_policy,
            source_scope,
            requested_ref: context.requested_ref,
            requested_resolved_ref: context.requested_resolved_ref,
            served_ref: context.request.repository.ref_selector.clone(),
            scope_stale: context.scoped_status.stale || context.stale_reason.is_some(),
            stale_reason: context.stale_reason,
            degraded_reason: context.degraded_reason,
            pending,
            cursor: checkpoint,
            direct_source_read_paths: feature_flag_paths(context.flags),
        },
    ))
}

async fn pending_work_and_checkpoint(
    store: &Arc<dyn KnowledgeStore>,
    base_status: &CodeRepositoryStatus,
    scoped_status: &CodeRepositoryStatus,
    freshness_target: &CodeRepositorySelector,
) -> Result<
    (
        CodeRepositoryPendingIndexWork,
        Option<CodeRepositoryFreshnessCursor>,
    ),
    ApiError,
> {
    let active_task = store
        .active_code_index_task(base_status.repository_id.clone())
        .await
        .map_err(storage_api_error)?;
    let active_matches_request = match active_task.as_ref() {
        Some(_) => active_index_matches_request(store, base_status, freshness_target).await?,
        None => false,
    };
    let queue = store
        .code_index_task_queue_status()
        .await
        .map_err(storage_api_error)?;
    let pending = CodeRepositoryPendingIndexWork::from_task_and_queue(
        active_task.as_ref(),
        active_matches_request,
        queue,
    );
    let checkpoint = scoped_freshness_checkpoint(
        store,
        scoped_status,
        active_task.as_ref(),
        active_matches_request,
    )
    .await?;

    Ok((pending, checkpoint))
}

async fn scoped_freshness_checkpoint(
    store: &Arc<dyn KnowledgeStore>,
    scoped_status: &CodeRepositoryStatus,
    active_task: Option<&crate::domain::CodeIndexTaskRecord>,
    active_matches_request: bool,
) -> Result<Option<CodeRepositoryFreshnessCursor>, ApiError> {
    let checkpoint = if active_matches_request {
        code_status_checkpoint(store, scoped_status, active_task).await?
    } else if let Some(scope) = scoped_status.last_indexed_scope_id.clone() {
        store
            .code_index_checkpoint(scope)
            .await
            .map_err(storage_api_error)?
    } else {
        None
    };

    Ok(checkpoint.map(|checkpoint| CodeRepositoryFreshnessCursor::from_checkpoint(&checkpoint)))
}

fn result_paths(results: &[CodeRetrievalHit]) -> Vec<String> {
    results
        .iter()
        .map(|hit| hit.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn feature_flag_paths(flags: &[CodeFeatureFlagGraph]) -> Vec<String> {
    flags
        .iter()
        .flat_map(|flag| flag.usages.iter().map(|usage| usage.path.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
