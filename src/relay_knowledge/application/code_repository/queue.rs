use std::sync::Arc;

use crate::{
    api::ApiError,
    domain::{
        CodeIndexRequest, CodeIndexResourceBudget, CodeRepositoryStatus, code_snapshot_scope_id,
    },
    storage::{CodeIndexTaskSeed, KnowledgeStore},
};

use super::support::{
    merged_filters, now_millis, previous_index_state_for_index, storage_api_error,
};

pub(super) async fn queue_worktree_overlay_index_task(
    store: &Arc<dyn KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
) -> Result<crate::domain::CodeIndexTaskRecord, ApiError> {
    let previous = previous_index_state_for_index(store, status, request).await?;
    let base_commit = previous.base_resolved_commit_sha.ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "worktree overlay for code repository '{}' requires a resolved HEAD base scope",
            status.alias
        ))
    })?;
    let path_filters = merged_filters(&status.path_filters, &request.repository.path_filters);
    let language_filters = merged_filters(
        &status.language_filters,
        &request.repository.language_filters,
    );
    let workspace_detection_json = serde_json::to_string(&request.workspace_detection)
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let payload_json = pinned_worktree_overlay_payload(request, &base_commit)
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let tree_hash = format!("worktree:pending:{base_commit}");
    let source_scope = code_snapshot_scope_id(
        &status.repository_id,
        &tree_hash,
        &path_filters,
        &language_filters,
    );
    let queued_at_ms = now_millis();
    let input_fingerprint = worktree_overlay_input_fingerprint(
        status,
        request,
        &base_commit,
        &path_filters,
        &language_filters,
        &workspace_detection_json,
        queued_at_ms,
    );
    store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: status.repository_id.clone(),
            alias: status.alias.clone(),
            ref_selector: base_commit.clone(),
            resolved_commit_sha: tree_hash.clone(),
            tree_hash,
            source_scope,
            path_filters,
            language_filters,
            mode: request.mode.clone(),
            input_fingerprint,
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json,
            now_ms: queued_at_ms,
        })
        .await
        .map_err(storage_api_error)
}

fn pinned_worktree_overlay_payload(
    request: &CodeIndexRequest,
    base_commit: &str,
) -> Result<String, serde_json::Error> {
    let mut payload = request.clone();
    payload.repository.ref_selector = base_commit.to_owned();
    serde_json::to_string(&payload)
}

fn worktree_overlay_input_fingerprint(
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
    base_commit: &str,
    path_filters: &[String],
    language_filters: &[String],
    workspace_detection_json: &str,
    queued_at_ms: u64,
) -> String {
    format!(
        "worktree:{}:{}:{}:{}:{}:{}:{}",
        status.repository_id,
        base_commit,
        serde_json::to_string(path_filters).unwrap_or_default(),
        serde_json::to_string(language_filters).unwrap_or_default(),
        request.repository.ref_selector,
        workspace_detection_json,
        queued_at_ms
    )
}
