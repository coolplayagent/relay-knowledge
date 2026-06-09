use std::sync::Arc;

use crate::{
    api::{ApiError, ApiMetadata, CodeRepositoryIndexResponse, RequestContext},
    domain::{
        CodeIndexMode, CodeIndexProgressSummary, CodeIndexRequest, CodeIndexResourceBudget,
        CodeIndexSummary, CodeRepositoryStatus,
        code_snapshot_expected_scope_id as expected_scope_id,
    },
    storage::KnowledgeStore,
};

use super::support::{
    degraded_file_count_for_fresh_index, fresh_full_index_probe, storage_api_error,
};

pub(super) async fn fresh_full_index_response(
    store: &Arc<dyn KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
    context: &RequestContext,
) -> Result<Option<CodeRepositoryIndexResponse>, ApiError> {
    if request.mode != CodeIndexMode::Full {
        return Ok(None);
    }
    if request.workspace_detection.enabled {
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
    let expected_source_scope = expected_scope_id(
        &status.repository_id,
        &probe.tree_hash,
        &scoped_status.path_filters,
        &scoped_status.language_filters,
    );
    if scoped_status.stale
        || scoped_status.tree_hash.as_deref() != Some(probe.tree_hash.as_str())
        || expected_source_scope.as_deref().is_some_and(|expected| {
            scoped_status.last_indexed_scope_id.as_deref() != Some(expected)
        })
    {
        return Ok(None);
    }
    let graph_version = store
        .current_graph_version()
        .await
        .map_err(storage_api_error)?;
    let source_scope = scoped_status
        .last_indexed_scope_id
        .clone()
        .unwrap_or_default();
    store
        .clear_code_workspace_state(scoped_status.repository_id.clone(), source_scope.clone())
        .await
        .map_err(storage_api_error)?;
    let software_projection = store
        .refresh_software_global_projection(source_scope.clone())
        .await
        .map_err(storage_api_error)?;
    let degraded_file_count = degraded_file_count_for_fresh_index(store, &scoped_status).await?;
    let summary = CodeIndexSummary {
        repository_id: scoped_status.repository_id.clone(),
        source_scope,
        resolved_commit_sha: probe.resolved_commit_sha,
        tree_hash: probe.tree_hash,
        indexed_file_count: scoped_status.indexed_file_count,
        changed_path_count: 0,
        skipped_unchanged_count: scoped_status.indexed_file_count,
        deleted_path_count: 0,
        symbol_count: scoped_status.symbol_count,
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
