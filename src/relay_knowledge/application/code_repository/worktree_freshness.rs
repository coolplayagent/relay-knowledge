use std::sync::Arc;

use crate::{
    api::ApiError,
    code::compute_worktree_overlay_identity,
    domain::{CodeRepositorySelector, CodeRepositoryStatus},
    storage::KnowledgeStore,
};

use super::support::{
    code_scope_matches_current_fact_version, registration_from_status, resolved_code_scope_status,
    run_blocking_code, storage_api_error,
};
use super::worktree_ref::worktree_overlay_base_commit;

pub(super) async fn ensure_worktree_overlay_matches_current_worktree(
    store: &Arc<dyn KnowledgeStore>,
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
) -> Result<(), ApiError> {
    let active_commit = status.last_indexed_commit.as_deref().ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "code repository '{}' has no active worktree overlay",
            status.alias
        ))
    })?;
    if active_commit.starts_with("filesystem:") {
        return Ok(());
    }
    let base_commit = worktree_overlay_base_commit(active_commit).ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "code repository '{}' has no active worktree overlay",
            status.alias
        ))
    })?;
    let overlay_scope = resolved_code_scope_status(store, status, selector)
        .await
        .map_err(|_| stale_worktree_overlay_error(status))?;
    let base_scope = store
        .code_repository_scope_status(
            status.alias.clone(),
            base_commit.to_owned(),
            overlay_scope.path_filters.clone(),
            overlay_scope.language_filters.clone(),
        )
        .await
        .map_err(storage_api_error)?
        .filter(code_scope_matches_current_fact_version)
        .ok_or_else(|| stale_worktree_overlay_error(status))?;
    let base_source_scope = base_scope
        .last_indexed_scope_id
        .clone()
        .ok_or_else(|| stale_worktree_overlay_error(status))?;
    let previous_hashes = store
        .code_file_fingerprints_for_scope(base_source_scope)
        .await
        .map_err(storage_api_error)?;
    let registration = registration_from_status(status);
    let selector = CodeRepositorySelector {
        repository: status.alias.clone(),
        ref_selector: "HEAD".to_owned(),
        path_filters: overlay_scope.path_filters,
        language_filters: overlay_scope.language_filters,
    };
    let expected_commit = active_commit.to_owned();
    let expected_tree_hash = overlay_scope.tree_hash.unwrap_or_default();
    let base_commit = base_commit.to_owned();
    let (actual_commit, actual_tree_hash) = run_blocking_code(move || {
        compute_worktree_overlay_identity(
            &registration,
            &selector,
            previous_hashes,
            Some(base_commit),
        )
    })
    .await?;

    if actual_commit == expected_commit && actual_tree_hash == expected_tree_hash {
        return Ok(());
    }

    Err(stale_worktree_overlay_error(status))
}

fn stale_worktree_overlay_error(status: &CodeRepositoryStatus) -> ApiError {
    ApiError::invalid_argument(format!(
        "code repository '{}' worktree overlay is stale; run repo index --ref worktree before querying --ref worktree",
        status.alias
    ))
}
