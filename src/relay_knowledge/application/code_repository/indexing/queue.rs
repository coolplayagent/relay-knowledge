use std::sync::Arc;

use crate::{
    api::ApiError,
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget, CodeRepositoryStatus,
        clean_git_commit_from_snapshot_identity, code_snapshot_scope_id,
    },
    storage::{CodeIndexTaskSeed, KnowledgeStore},
};

use super::state::{fresh_full_index_probe, previous_index_state_for_index};
use crate::application::code_repository::{
    clock::now_millis, errors::storage_api_error, scope::merged_filters,
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

pub(super) async fn queue_incremental_index_task(
    store: &Arc<dyn KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
) -> Result<crate::domain::CodeIndexTaskRecord, ApiError> {
    let CodeIndexMode::Incremental { head_ref, .. } = &request.mode else {
        return Err(ApiError::invalid_argument(
            "incremental task queue requires incremental index mode",
        ));
    };
    let previous = previous_index_state_for_index(store, status, request).await?;
    let base_commit = previous.base_resolved_commit_sha.ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "incremental update for code repository '{}' requires a resolved base commit",
            status.alias
        ))
    })?;
    let mut head_selector = request.repository.clone();
    head_selector.ref_selector = head_ref.clone();
    let head = fresh_full_index_probe(status, &head_selector).await?;
    if status
        .last_indexed_commit
        .as_deref()
        .is_some_and(|identity| identity.starts_with("worktree:"))
        && clean_git_commit_from_snapshot_identity(
            status.last_indexed_commit.as_deref().unwrap_or_default(),
        ) == Some(head.resolved_commit_sha.as_str())
    {
        return Err(ApiError::invalid_argument(format!(
            "code repository '{}' still points at a worktree overlay whose clean base is already {}; create a commit first or query the worktree scope explicitly",
            status.alias, head.resolved_commit_sha
        )));
    }
    let pinned_mode =
        CodeIndexMode::incremental(base_commit.clone(), head.resolved_commit_sha.clone())
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let mut pinned_request = request.clone();
    pinned_request.mode = pinned_mode.clone();
    let payload_json = serde_json::to_string(&pinned_request)
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let workspace_detection_json = serde_json::to_string(&request.workspace_detection)
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let source_scope = code_snapshot_scope_id(
        &status.repository_id,
        &head.tree_hash,
        &head.path_filters,
        &head.language_filters,
    );
    let input_fingerprint = format!(
        "incremental:{}:{}:{}:{}:{}",
        status.repository_id,
        base_commit,
        head.resolved_commit_sha,
        source_scope,
        workspace_detection_json
    );

    store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: status.repository_id.clone(),
            alias: status.alias.clone(),
            ref_selector: request.repository.ref_selector.clone(),
            resolved_commit_sha: head.resolved_commit_sha,
            tree_hash: head.tree_hash,
            source_scope,
            path_filters: head.path_filters,
            language_filters: head.language_filters,
            mode: pinned_mode,
            input_fingerprint,
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json,
            now_ms: now_millis(),
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
