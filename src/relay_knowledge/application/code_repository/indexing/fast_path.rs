use std::sync::Arc;

use crate::{
    api::{ApiError, ApiMetadata, CodeRepositoryIndexResponse, RequestContext},
    domain::{
        CodeIncrementalSummaryReceipt, CodeIndexCheckpoint, CodeIndexMode,
        CodeIndexProgressSummary, CodeIndexRequest, CodeIndexResourceBudget, CodeIndexSummary,
        CodeRepositorySelector, CodeRepositoryStatus, FreshnessPolicy, SoftwareGlobalKind,
        SoftwareGlobalRequest, code_snapshot_scope_id_with_workspace_detection,
    },
    storage::{KnowledgeStore, StorageError},
};

use super::state::{degraded_file_count_for_fresh_index, fresh_full_index_probe};
use super::task::CodeIndexTaskLeaseContext;
use crate::application::code_repository::errors::storage_api_error;

pub(super) async fn fresh_full_index_response(
    store: &Arc<dyn KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
    context: &RequestContext,
) -> Result<Option<CodeRepositoryIndexResponse>, ApiError> {
    if request.mode != CodeIndexMode::Full {
        return Ok(None);
    }
    let probe = fresh_full_index_probe(status, &request.repository).await?;
    let scoped_status = store
        .code_repository_scope_status(
            request.repository.repository.clone(),
            probe.resolved_commit_sha.clone(),
            probe.path_filters.clone(),
            probe.language_filters.clone(),
        )
        .await
        .map_err(storage_api_error)?;
    let Some(scoped_status) = scoped_status else {
        return Ok(None);
    };
    let expected_source_scope = code_snapshot_scope_id_with_workspace_detection(
        &status.repository_id,
        &probe.tree_hash,
        &scoped_status.path_filters,
        &scoped_status.language_filters,
        &request.workspace_detection,
    );
    if scoped_status.stale
        || scoped_status.tree_hash.as_deref() != Some(probe.tree_hash.as_str())
        || scoped_status.last_indexed_scope_id.as_deref() != Some(expected_source_scope.as_str())
    {
        return Ok(None);
    }
    let source_scope = scoped_status
        .last_indexed_scope_id
        .clone()
        .unwrap_or_default();
    if store
        .code_index_checkpoint(source_scope.clone())
        .await
        .map_err(storage_api_error)?
        .is_some_and(|checkpoint| checkpoint.state != "completed")
    {
        return Ok(None);
    }
    if !request.workspace_detection.enabled
        && store
            .code_repository_auto_workspace_state_exists(scoped_status.repository_id.clone())
            .await
            .map_err(storage_api_error)?
    {
        return Ok(None);
    }
    let projection_request = SoftwareGlobalRequest::new(
        CodeRepositorySelector::new(
            scoped_status.repository_id.clone(),
            probe.resolved_commit_sha.clone(),
            scoped_status.path_filters.clone(),
            scoped_status.language_filters.clone(),
        )
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?,
        SoftwareGlobalKind::All,
        FreshnessPolicy::WaitUntilFresh,
        1,
    )
    .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let software_projection = store
        .software_global_projection_for_scope(source_scope.clone(), projection_request)
        .await
        .map_err(storage_api_error)?;
    if software_projection.status.stale
        || software_projection.status.repository_id != scoped_status.repository_id
        || software_projection.status.source_scope != source_scope
    {
        return Ok(None);
    }
    let graph_version = store
        .current_graph_version()
        .await
        .map_err(storage_api_error)?;
    let degraded_file_count = degraded_file_count_for_fresh_index(store, &scoped_status).await?;
    let generation_counts = store
        .code_repository_scope_symbol_generation_counts(source_scope.clone())
        .await
        .map_err(storage_api_error)?;
    let summary = CodeIndexSummary {
        repository_id: scoped_status.repository_id.clone(),
        source_scope,
        base_resolved_commit_sha: None,
        resolved_commit_sha: probe.resolved_commit_sha,
        tree_hash: probe.tree_hash,
        indexed_file_count: scoped_status.indexed_file_count,
        changed_path_count: 0,
        skipped_unchanged_count: scoped_status.indexed_file_count,
        deleted_path_count: 0,
        symbol_count: scoped_status.symbol_count,
        handwritten_symbol_count: generation_counts.handwritten_symbol_count,
        generated_symbol_count: generation_counts.generated_symbol_count,
        reference_count: scoped_status.reference_count,
        chunk_count: scoped_status.chunk_count,
        degraded_file_count,
        progress: CodeIndexProgressSummary {
            git_file_count: scoped_status.indexed_file_count,
            blob_read_count: 0,
            parsed_file_count: 0,
            sqlite_write_count: 0,
            skipped_file_count: scoped_status.indexed_file_count,
            degraded_file_count,
            batch_count: 0,
            checkpoint_file_count: scoped_status.indexed_file_count,
            resource_budget: CodeIndexResourceBudget::default(),
        },
    };

    Ok(Some(CodeRepositoryIndexResponse {
        metadata: ApiMetadata::graph_only(context, graph_version),
        scope: crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            request.repository.ref_selector.clone(),
        ),
        summary,
        status: CodeRepositoryStatus {
            degraded_reason: scoped_status
                .degraded_reason
                .or(software_projection.status.last_error),
            ..scoped_status
        },
    }))
}

pub(super) async fn published_task_response(
    store: &Arc<dyn KnowledgeStore>,
    repository_status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
    context: &RequestContext,
    lease: &CodeIndexTaskLeaseContext,
) -> Result<CodeRepositoryIndexResponse, ApiError> {
    let scoped_status = store
        .code_repository_scope_status(
            lease.publication_fence.repository_id.clone(),
            lease.resolved_commit_sha.clone(),
            lease.path_filters.clone(),
            lease.language_filters.clone(),
        )
        .await
        .map_err(storage_api_error)?
        .filter(|status| {
            status.last_indexed_scope_id.as_deref() == Some(lease.source_scope.as_str())
                && status.tree_hash.as_deref() == Some(lease.tree_hash.as_str())
                && status.state == "fresh"
                && !status.stale
        })
        .ok_or_else(|| {
            ApiError::storage_unavailable(format!(
                "published receipt for task '{}' does not match its active code scope",
                lease.task_id
            ))
        })?;
    if !request.workspace_detection.enabled {
        store
            .clear_code_workspace_state_with_fence(
                lease.publication_fence.repository_id.clone(),
                lease.source_scope.clone(),
                lease.publication_fence.clone(),
            )
            .await
            .map_err(storage_api_error)?;
    }
    let projection_request = crate::domain::SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            repository_status.alias.clone(),
            lease.resolved_commit_sha.clone(),
            lease.path_filters.clone(),
            lease.language_filters.clone(),
        )
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?,
        crate::domain::SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::WaitUntilFresh,
        1,
    )
    .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let software = store
        .software_global_projection_for_scope(lease.source_scope.clone(), projection_request)
        .await
        .map_err(storage_api_error)?;
    if software.status.stale
        || software.status.repository_id != lease.publication_fence.repository_id
        || software.status.source_scope != lease.source_scope
    {
        return Err(ApiError::storage_unavailable(format!(
            "published receipt for task '{}' does not match its software projection",
            lease.task_id
        )));
    }
    let graph_version = store
        .current_graph_version()
        .await
        .map_err(storage_api_error)?;
    let generation_counts = store
        .code_repository_scope_symbol_generation_counts(lease.source_scope.clone())
        .await
        .map_err(storage_api_error)?;
    let checkpoint = store
        .code_index_checkpoint(lease.source_scope.clone())
        .await
        .map_err(storage_api_error)?;
    let incremental =
        incremental_recovery_receipt(checkpoint.as_ref(), lease).map_err(storage_api_error)?;
    let degraded_file_count = match incremental.as_ref() {
        Some(receipt) => receipt.degraded_file_count,
        None => degraded_file_count_for_fresh_index(store, &scoped_status).await?,
    };
    let summary = CodeIndexSummary {
        repository_id: scoped_status.repository_id.clone(),
        source_scope: lease.source_scope.clone(),
        base_resolved_commit_sha: incremental
            .as_ref()
            .map(|receipt| receipt.base_resolved_commit_sha.clone()),
        resolved_commit_sha: lease.resolved_commit_sha.clone(),
        tree_hash: lease.tree_hash.clone(),
        indexed_file_count: scoped_status.indexed_file_count,
        changed_path_count: incremental
            .as_ref()
            .map_or(0, |receipt| receipt.changed_path_count),
        skipped_unchanged_count: incremental
            .as_ref()
            .map_or(scoped_status.indexed_file_count, |receipt| {
                receipt.skipped_unchanged_count
            }),
        deleted_path_count: incremental
            .as_ref()
            .map_or(0, |receipt| receipt.deleted_path_count),
        symbol_count: scoped_status.symbol_count,
        handwritten_symbol_count: generation_counts.handwritten_symbol_count,
        generated_symbol_count: generation_counts.generated_symbol_count,
        reference_count: scoped_status.reference_count,
        chunk_count: scoped_status.chunk_count,
        degraded_file_count,
        progress: incremental.as_ref().map_or_else(
            || {
                publication_recovery_progress(
                    scoped_status.indexed_file_count,
                    lease.resource_budget,
                )
            },
            |receipt| incremental_recovery_progress(receipt, lease.resource_budget),
        ),
    };
    Ok(CodeRepositoryIndexResponse {
        metadata: ApiMetadata::graph_only(context, graph_version),
        scope: crate::api::CodeRepositoryScopeMetadata::from_status(
            &scoped_status,
            &request.repository,
            request.repository.ref_selector.clone(),
        ),
        summary,
        status: CodeRepositoryStatus {
            degraded_reason: scoped_status
                .degraded_reason
                .clone()
                .or(software.status.last_error),
            ..scoped_status
        },
    })
}

fn incremental_recovery_receipt(
    checkpoint: Option<&CodeIndexCheckpoint>,
    lease: &CodeIndexTaskLeaseContext,
) -> Result<Option<CodeIncrementalSummaryReceipt>, StorageError> {
    let Some((checkpoint, receipt)) = checkpoint.and_then(|checkpoint| {
        checkpoint
            .incremental_summary
            .as_ref()
            .map(|receipt| (checkpoint, receipt))
    }) else {
        return Ok(None);
    };
    let checkpoint_matches = checkpoint.repository_id == lease.publication_fence.repository_id
        && checkpoint.source_scope == lease.source_scope
        && checkpoint.resolved_commit_sha == lease.resolved_commit_sha
        && checkpoint.tree_hash == lease.tree_hash
        && checkpoint.path_filters == lease.path_filters
        && checkpoint.language_filters == lease.language_filters
        && checkpoint.resource_budget == lease.resource_budget;
    if !checkpoint_matches {
        return Err(StorageError::Invariant(format!(
            "durable incremental publication receipt for task '{}' does not match its live checkpoint",
            lease.task_id
        )));
    }
    if receipt.task_id != lease.task_id {
        // A content-addressed scope can be adopted by a later task without rewriting its facts.
        // The old task's metrics remain historical evidence, not the adopting task's summary.
        return Ok(None);
    }
    Ok(Some(receipt.clone()))
}

fn incremental_recovery_progress(
    receipt: &CodeIncrementalSummaryReceipt,
    resource_budget: CodeIndexResourceBudget,
) -> CodeIndexProgressSummary {
    CodeIndexProgressSummary {
        git_file_count: receipt.changed_path_count,
        blob_read_count: receipt.blob_read_count,
        parsed_file_count: receipt.parsed_file_count,
        sqlite_write_count: receipt.sqlite_write_count,
        skipped_file_count: receipt.skipped_unchanged_count,
        degraded_file_count: receipt.degraded_file_count,
        batch_count: receipt.batch_count,
        checkpoint_file_count: receipt.parsed_file_count,
        resource_budget,
    }
}

fn publication_recovery_progress(
    indexed_file_count: usize,
    resource_budget: CodeIndexResourceBudget,
) -> CodeIndexProgressSummary {
    CodeIndexProgressSummary {
        git_file_count: 0,
        blob_read_count: 0,
        parsed_file_count: 0,
        sqlite_write_count: 0,
        skipped_file_count: indexed_file_count,
        degraded_file_count: 0,
        batch_count: 0,
        checkpoint_file_count: indexed_file_count,
        resource_budget,
    }
}

#[cfg(test)]
#[path = "fast_path_tests.rs"]
mod fast_path_tests;
