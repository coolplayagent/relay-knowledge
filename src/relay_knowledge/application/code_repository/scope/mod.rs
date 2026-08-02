//! Owns indexed scope resolution, filter compatibility, and active-scope matching.

use std::path::PathBuf;

use crate::{
    api::ApiError,
    code::{repository_uses_filesystem_source, resolve_repository_ref_with_filters},
    domain::{
        CodeFeatureFlagRequest, CodeIndexTaskRecord, CodeRepositorySelector, CodeRepositoryStatus,
        CodeRetrievalRequest, code_snapshot_expected_scope_id,
        code_snapshot_scope_is_fact_versioned,
    },
};

use super::{blocking::run_blocking_code, errors::storage_api_error};

pub(super) async fn retrieval_request_at_indexed_ref(
    mut request: CodeRetrievalRequest,
    status: &CodeRepositoryStatus,
) -> Result<CodeRetrievalRequest, ApiError> {
    request.repository.ref_selector = indexed_commit_for_selector(
        status,
        &request.repository,
        request.repository.ref_selector.clone(),
    )
    .await?;

    Ok(request)
}

pub(super) async fn feature_flag_request_at_indexed_ref(
    mut request: CodeFeatureFlagRequest,
    status: &CodeRepositoryStatus,
) -> Result<CodeFeatureFlagRequest, ApiError> {
    request.repository.ref_selector = indexed_commit_for_selector(
        status,
        &request.repository,
        request.repository.ref_selector.clone(),
    )
    .await?;

    Ok(request)
}

pub(super) async fn resolved_code_scope_status(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
) -> Result<CodeRepositoryStatus, ApiError> {
    let path_filters = merged_filters(&status.path_filters, &selector.path_filters);
    let language_filters = merged_filters(&status.language_filters, &selector.language_filters);
    let exact_scope = store
        .code_repository_scope_status(
            selector.repository.clone(),
            selector.ref_selector.clone(),
            path_filters,
            language_filters,
        )
        .await
        .map_err(storage_api_error)?
        .filter(code_scope_matches_current_fact_version);
    let scoped_status = match exact_scope {
        Some(status) => Some(status),
        None if (!selector.path_filters.is_empty() || !selector.language_filters.is_empty())
            && selector_filters_fit_indexed_scope(status, selector) =>
        {
            store
                .code_repository_scope_status(
                    selector.repository.clone(),
                    selector.ref_selector.clone(),
                    status.path_filters.clone(),
                    status.language_filters.clone(),
                )
                .await
                .map_err(storage_api_error)?
                .filter(code_scope_matches_current_fact_version)
        }
        None => None,
    };
    scoped_status.ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "code repository '{}' has no index for ref {} and requested filters at the current code fact version",
            selector.repository, selector.ref_selector
        ))
    })
}

pub(super) async fn latest_compatible_code_scope_status(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    selector: &CodeRepositorySelector,
) -> Result<Option<CodeRepositoryStatus>, ApiError> {
    let status = store
        .latest_code_repository_scope_status(
            selector.repository.clone(),
            selector.path_filters.clone(),
            selector.language_filters.clone(),
        )
        .await
        .map_err(storage_api_error)?;

    Ok(status.filter(code_scope_matches_current_fact_version))
}

pub(super) fn code_scope_matches_current_fact_version(status: &CodeRepositoryStatus) -> bool {
    let Some(source_scope) = status.last_indexed_scope_id.as_deref() else {
        return false;
    };
    if !code_snapshot_scope_is_fact_versioned(source_scope) {
        return true;
    }
    let Some(tree_hash) = status.tree_hash.as_deref() else {
        return false;
    };

    code_snapshot_expected_scope_id(
        &status.repository_id,
        tree_hash,
        &status.path_filters,
        &status.language_filters,
    )
    .is_some_and(|expected| expected == source_scope)
}

pub(super) fn indexed_source_scope(status: &CodeRepositoryStatus) -> Option<String> {
    status.last_indexed_scope_id.clone()
}

pub(super) fn missing_indexed_source_scope_error(status: &CodeRepositoryStatus) -> ApiError {
    ApiError::invalid_argument(format!(
        "code repository '{}' does not have an indexed source scope",
        status.alias
    ))
}

pub(super) fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}

fn selector_filters_fit_indexed_scope(
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
) -> bool {
    requested_paths_fit_indexed_scope(&status.path_filters, &selector.path_filters)
        && requested_languages_fit_indexed_scope(
            &status.language_filters,
            &selector.language_filters,
        )
}

fn requested_paths_fit_indexed_scope(
    indexed_filters: &[String],
    selector_filters: &[String],
) -> bool {
    selector_filters.is_empty()
        || indexed_filters.is_empty()
        || selector_filters.iter().all(|selector_filter| {
            indexed_filters
                .iter()
                .any(|indexed_filter| path_filter_covers(indexed_filter, selector_filter))
        })
}

fn requested_languages_fit_indexed_scope(
    indexed_filters: &[String],
    selector_filters: &[String],
) -> bool {
    selector_filters.is_empty()
        || indexed_filters.is_empty()
        || selector_filters
            .iter()
            .all(|selector_filter| indexed_filters.contains(selector_filter))
}

pub(super) async fn active_index_matches_request(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
) -> Result<bool, ApiError> {
    let Some(task) = store
        .active_code_index_task(status.repository_id.clone())
        .await
        .map_err(storage_api_error)?
    else {
        return Ok(false);
    };

    if !active_task_filters_cover_requested_scope(status, &task, selector) {
        return Ok(false);
    }

    if task.resolved_commit_sha == selector.ref_selector {
        return Ok(true);
    }

    active_non_git_index_matches_selector(status, &task, selector).await
}

async fn active_non_git_index_matches_selector(
    status: &CodeRepositoryStatus,
    task: &CodeIndexTaskRecord,
    selector: &CodeRepositorySelector,
) -> Result<bool, ApiError> {
    if !selector.ref_selector.starts_with("filesystem:") {
        return Ok(false);
    }

    let root = PathBuf::from(status.root_path.clone());
    let task_ref_selector = task.ref_selector.clone();
    let task_resolved_commit = task.resolved_commit_sha.clone();
    let task_path_filters = task.path_filters.clone();
    let task_language_filters = task.language_filters.clone();
    let selector_resolved_commit = selector.ref_selector.clone();
    let selector_path_filters = merged_filters(&status.path_filters, &selector.path_filters);
    let selector_language_filters =
        merged_filters(&status.language_filters, &selector.language_filters);

    run_blocking_code(move || {
        if !repository_uses_filesystem_source(&root)? {
            return Ok(false);
        }

        let live_task_commit = resolve_repository_ref_with_filters(
            root.clone(),
            &task_ref_selector,
            &task_path_filters,
            &task_language_filters,
        )?;
        if live_task_commit != task_resolved_commit {
            return Ok(false);
        }

        let live_selector_commit = resolve_repository_ref_with_filters(
            root,
            &task_ref_selector,
            &selector_path_filters,
            &selector_language_filters,
        )?;

        Ok(live_selector_commit == selector_resolved_commit)
    })
    .await
}

fn active_task_filters_cover_requested_scope(
    status: &CodeRepositoryStatus,
    task: &CodeIndexTaskRecord,
    selector: &CodeRepositorySelector,
) -> bool {
    active_languages_cover_requested_scope(
        &status.language_filters,
        &task.language_filters,
        &selector.language_filters,
    ) && active_paths_cover_requested_scope(
        &status.path_filters,
        &task.path_filters,
        &selector.path_filters,
    )
}

fn active_paths_cover_requested_scope(
    registration_filters: &[String],
    task_filters: &[String],
    selector_filters: &[String],
) -> bool {
    if !requested_paths_fit_indexed_scope(registration_filters, selector_filters) {
        return false;
    }
    let task_selector_filters =
        filters_without_registration_scope(task_filters, registration_filters);
    if selector_filters.is_empty() {
        return task_selector_filters.is_empty();
    }
    task_selector_filters.is_empty()
        || requested_paths_fit_indexed_scope(&task_selector_filters, selector_filters)
}

fn active_languages_cover_requested_scope(
    registration_filters: &[String],
    task_filters: &[String],
    selector_filters: &[String],
) -> bool {
    if !requested_languages_fit_indexed_scope(registration_filters, selector_filters) {
        return false;
    }
    let task_selector_filters =
        filters_without_registration_scope(task_filters, registration_filters);
    if selector_filters.is_empty() {
        return task_selector_filters.is_empty();
    }
    task_selector_filters.is_empty()
        || requested_languages_fit_indexed_scope(&task_selector_filters, selector_filters)
}

fn filters_without_registration_scope(
    task_filters: &[String],
    registration_filters: &[String],
) -> Vec<String> {
    task_filters
        .iter()
        .filter(|filter| !registration_filters.contains(filter))
        .cloned()
        .collect()
}

fn path_filter_covers(indexed_filter: &str, selector_filter: &str) -> bool {
    let indexed_filter = normalize_path_filter(indexed_filter);
    let selector_filter = normalize_path_filter(selector_filter);
    indexed_filter == "."
        || (!indexed_filter.is_empty()
            && !selector_filter.is_empty()
            && (selector_filter == indexed_filter
                || selector_filter.starts_with(&format!("{indexed_filter}/"))))
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

pub(super) async fn indexed_commit_for_selector(
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
    ref_selector: String,
) -> Result<String, ApiError> {
    if ref_selector == "worktree" {
        if is_worktree_overlay(status) {
            return status.last_indexed_commit.clone().ok_or_else(|| {
                ApiError::invalid_argument(format!(
                    "code repository '{}' has no active worktree overlay",
                    status.alias
                ))
            });
        }
        let root = PathBuf::from(status.root_path.clone());
        if run_blocking_code(move || repository_uses_filesystem_source(&root)).await? {
            return resolve_code_ref_for_selector(status, selector, ref_selector).await;
        }
        return Err(ApiError::invalid_argument(format!(
            "code repository '{}' has no active worktree overlay",
            status.alias
        )));
    }

    resolve_code_ref_for_selector(status, selector, ref_selector).await
}

fn is_worktree_overlay(status: &CodeRepositoryStatus) -> bool {
    status
        .last_indexed_commit
        .as_deref()
        .is_some_and(|value| value.starts_with("worktree:"))
        || status
            .tree_hash
            .as_deref()
            .is_some_and(|value| value.starts_with("worktree:"))
}

pub(super) async fn resolve_code_ref_for_selector(
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
    ref_selector: String,
) -> Result<String, ApiError> {
    let root = PathBuf::from(status.root_path.clone());
    let path_filters = merged_filters(&status.path_filters, &selector.path_filters);
    let language_filters = merged_filters(&status.language_filters, &selector.language_filters);
    let active_commit = status.last_indexed_commit.clone();
    let active_path_filters = status.path_filters.clone();
    let active_language_filters = status.language_filters.clone();
    let selector_fits_active_scope = !ref_selector.starts_with("filesystem:")
        && selector_filters_fit_indexed_scope(status, selector);

    run_blocking_code(move || {
        if selector_fits_active_scope
            && let Some(active_commit) = active_commit
            && repository_uses_filesystem_source(&root)?
        {
            let active_live_commit = resolve_repository_ref_with_filters(
                root.clone(),
                &ref_selector,
                &active_path_filters,
                &active_language_filters,
            )?;
            if active_live_commit == active_commit {
                return Ok(active_commit);
            }
        }
        resolve_repository_ref_with_filters(root, &ref_selector, &path_filters, &language_filters)
    })
    .await
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
