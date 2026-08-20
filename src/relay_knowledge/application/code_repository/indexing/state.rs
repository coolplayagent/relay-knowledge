//! Owns persisted index-state inspection and index reuse decisions.

use std::path::PathBuf;

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse, ErrorKind},
    code::{
        first_parent_ancestors_bounded, historical_reuse_diff_fits_budget, prepare_full_index_plan,
        repository_uses_filesystem_source, resolve_repository_snapshot_with_filters,
    },
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget, CodeIndexTaskRecord,
        CodeRepositorySelector, CodeRepositoryStatus,
    },
};

use super::super::{
    blocking::run_blocking_code,
    errors::storage_api_error,
    repository::registration_from_status,
    scope::{
        code_scope_matches_current_fact_version, merged_filters, resolve_code_ref_for_selector,
    },
};

pub(super) const RETAIN_RECENT_CODE_SCOPES: usize = 2;
const FULL_INDEX_ANCESTOR_PROBE_LIMIT: usize = 10;

pub(super) enum FullIndexReusePlan {
    ActiveTask(CodeIndexTaskRecord),
    Incremental(CodeIndexRequest),
    Full,
}

pub(super) fn historical_reuse_base_became_unavailable(error: &ApiError) -> bool {
    error.error_kind == ErrorKind::StorageUnavailable
        && error
            .message
            .contains("has no compatible non-retiring scope")
}

pub(super) struct PreviousIndexState {
    pub(super) fingerprints: Vec<crate::domain::CodeFileFingerprint>,
    pub(super) base_resolved_commit_sha: Option<String>,
}

pub(super) struct FreshFullIndexProbe {
    pub(super) resolved_commit_sha: String,
    pub(super) tree_hash: String,
    pub(super) path_filters: Vec<String>,
    pub(super) language_filters: Vec<String>,
}

pub(super) fn requested_index_ref_for_response(request: &CodeIndexRequest) -> String {
    if request.mode == CodeIndexMode::WorktreeOverlay {
        "worktree".to_owned()
    } else {
        request.repository.ref_selector.clone()
    }
}

pub(super) async fn fresh_full_index_probe(
    status: &CodeRepositoryStatus,
    selector: &CodeRepositorySelector,
) -> Result<FreshFullIndexProbe, ApiError> {
    let registration = registration_from_status(status);
    let selector = selector.clone();
    let root = PathBuf::from(status.root_path.clone());
    run_blocking_code(move || {
        if selector.ref_selector.starts_with("filesystem:")
            || repository_uses_filesystem_source(&root)?
        {
            let plan = prepare_full_index_plan(
                registration,
                selector,
                CodeIndexResourceBudget::default(),
            )?;
            let session = plan.session();
            return Ok(FreshFullIndexProbe {
                resolved_commit_sha: session.resolved_commit_sha,
                tree_hash: session.tree_hash,
                path_filters: session.path_filters,
                language_filters: session.language_filters,
            });
        }

        let path_filters = merged_filters(&registration.path_filters, &selector.path_filters);
        let language_filters =
            merged_filters(&registration.language_filters, &selector.language_filters);
        let (resolved_commit_sha, tree_hash) = resolve_repository_snapshot_with_filters(
            &root,
            &selector.ref_selector,
            &path_filters,
            &language_filters,
        )?;

        Ok(FreshFullIndexProbe {
            resolved_commit_sha,
            tree_hash,
            path_filters,
            language_filters,
        })
    })
    .await
}

pub(super) async fn degraded_file_count_for_fresh_index(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    scoped_status: &CodeRepositoryStatus,
) -> Result<usize, ApiError> {
    if let Some(count) = degraded_file_count_from_status(scoped_status) {
        return Ok(count);
    }
    let report = store
        .code_repository_report(scoped_status.repository_id.clone())
        .await
        .map_err(storage_api_error)?;

    Ok(report.degraded_file_count)
}

fn degraded_file_count_from_status(status: &CodeRepositoryStatus) -> Option<usize> {
    let reason = status.degraded_reason.as_deref()?;
    let (count, rest) = reason.split_once(' ')?;
    (rest == "file(s) degraded during code indexing")
        .then(|| count.parse().ok())
        .flatten()
}

pub(super) fn index_start_from_completed(
    response: CodeRepositoryIndexResponse,
    task: Option<crate::domain::CodeIndexTaskRecord>,
) -> CodeRepositoryIndexStartResponse {
    CodeRepositoryIndexStartResponse {
        metadata: response.metadata,
        scope: response.scope,
        summary: Some(response.summary),
        status: response.status,
        task,
        checkpoint: None,
    }
}

pub(super) async fn previous_index_state_for_index(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
) -> Result<PreviousIndexState, ApiError> {
    let base_ref = match &request.mode {
        CodeIndexMode::Incremental { base_ref, .. } => base_ref.as_str(),
        CodeIndexMode::WorktreeOverlay => request.repository.ref_selector.as_str(),
        CodeIndexMode::Full => {
            let fingerprints = store
                .code_file_fingerprints(status.repository_id.clone())
                .await
                .map_err(storage_api_error)?;
            return Ok(PreviousIndexState {
                fingerprints,
                base_resolved_commit_sha: status.last_indexed_commit.clone(),
            });
        }
    };
    let base_commit =
        resolve_code_ref_for_selector(status, &request.repository, base_ref.to_owned()).await?;
    let path_filters = merged_filters(&status.path_filters, &request.repository.path_filters);
    let language_filters = merged_filters(
        &status.language_filters,
        &request.repository.language_filters,
    );
    let base_scope = store
        .code_repository_scope_status(
            request.repository.repository.clone(),
            base_commit.clone(),
            path_filters,
            language_filters,
        )
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            if request.mode == CodeIndexMode::WorktreeOverlay {
                return ApiError::invalid_argument(format!(
                    "worktree overlay for code repository '{}' requires an indexed {} base scope; run repo index --ref {} before repo index --ref worktree",
                    status.alias, base_ref, base_ref
                ));
            }
            ApiError::invalid_argument(format!(
                "incremental base ref '{}' resolves to {}, but code repository '{}' has no matching indexed base scope; run repo index --ref {} before repo update",
                base_ref, base_commit, status.alias, base_ref
            ))
        })?;
    if base_scope.stale {
        if request.mode == CodeIndexMode::WorktreeOverlay {
            return Err(ApiError::invalid_argument(format!(
                "worktree overlay base ref '{}' resolves to a stale indexed scope {}; refresh or reindex the base before repo index --ref worktree",
                base_ref,
                base_scope
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped")
            )));
        }
        return Err(ApiError::invalid_argument(format!(
            "incremental base ref '{}' resolves to a stale indexed scope {}; refresh or reindex the base before repo update",
            base_ref,
            base_scope
                .last_indexed_scope_id
                .as_deref()
                .unwrap_or("unscoped")
        )));
    }
    let source_scope = base_scope.last_indexed_scope_id.clone().ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "incremental base ref '{}' has no persisted source scope",
            base_ref
        ))
    })?;
    if !code_scope_matches_current_fact_version(&base_scope) {
        if request.mode == CodeIndexMode::WorktreeOverlay {
            return Err(ApiError::invalid_argument(format!(
                "worktree overlay base ref '{}' resolves to scope '{}' built with an older code fact version; run repo index --ref {} before repo index --ref worktree",
                base_ref, source_scope, base_ref
            )));
        }
        return Err(ApiError::invalid_argument(format!(
            "incremental base ref '{}' resolves to scope '{}' built with an older code fact version; run repo index --ref {} before repo update",
            base_ref, source_scope, base_ref
        )));
    }

    let fingerprints = store
        .code_file_fingerprints_for_scope(source_scope)
        .await
        .map_err(storage_api_error)?;
    Ok(PreviousIndexState {
        fingerprints,
        base_resolved_commit_sha: Some(base_commit),
    })
}

pub(super) async fn plan_full_index_reuse(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
) -> Result<FullIndexReusePlan, ApiError> {
    if request.mode != CodeIndexMode::Full {
        return Ok(FullIndexReusePlan::Full);
    }
    let root = PathBuf::from(&status.root_path);
    if run_blocking_code({
        let root = root.clone();
        move || repository_uses_filesystem_source(root)
    })
    .await?
    {
        return Ok(FullIndexReusePlan::Full);
    }

    let mut target = fresh_full_index_probe(status, &request.repository).await?;
    let target_commit = target.resolved_commit_sha.clone();
    let target_session =
        run_blocking_code({
            let registration = registration_from_status(status);
            let mut selector = request.repository.clone();
            selector.ref_selector = target_commit.clone();
            move || {
                Ok(prepare_full_index_plan(
                    registration,
                    selector,
                    CodeIndexResourceBudget::default(),
                )?
                .session())
            }
        })
        .await?;
    target.path_filters.clone_from(&target_session.path_filters);
    target
        .language_filters
        .clone_from(&target_session.language_filters);
    if let Some(task) = active_index_task_for_target(store, status, request, &target).await? {
        return Ok(FullIndexReusePlan::ActiveTask(task));
    }
    let ancestors = run_blocking_code({
        let root = root.clone();
        let target_commit = target_commit.clone();
        move || {
            first_parent_ancestors_bounded(&root, &target_commit, FULL_INDEX_ANCESTOR_PROBE_LIMIT)
        }
    })
    .await?;
    let path_filters = target_session.path_filters;
    let language_filters = target_session.language_filters;
    for ancestor in ancestors {
        let Some(base_scope) = store
            .code_repository_scope_status(
                request.repository.repository.clone(),
                ancestor.clone(),
                path_filters.clone(),
                language_filters.clone(),
            )
            .await
            .map_err(storage_api_error)?
        else {
            continue;
        };
        if base_scope.stale || !code_scope_matches_current_fact_version(&base_scope) {
            continue;
        }
        if !scope_filters_match_incremental_clone(&base_scope, &path_filters, &language_filters) {
            continue;
        }
        let fits_budget = run_blocking_code({
            let root = root.clone();
            let ancestor = ancestor.clone();
            let target_commit = target_commit.clone();
            let path_filters = path_filters.clone();
            let language_filters = language_filters.clone();
            move || {
                historical_reuse_diff_fits_budget(
                    root,
                    &ancestor,
                    &target_commit,
                    &path_filters,
                    &language_filters,
                )
            }
        })
        .await?;
        if !fits_budget {
            return Ok(FullIndexReusePlan::Full);
        }

        let mut incremental = request.clone();
        incremental.repository.ref_selector = target_commit.clone();
        incremental.repository.path_filters = path_filters.clone();
        incremental.repository.language_filters = language_filters.clone();
        incremental.mode = CodeIndexMode::incremental(ancestor, target_commit.clone())
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        return Ok(FullIndexReusePlan::Incremental(incremental));
    }

    Ok(FullIndexReusePlan::Full)
}

fn scope_filters_match_incremental_clone(
    scope: &CodeRepositoryStatus,
    path_filters: &[String],
    language_filters: &[String],
) -> bool {
    canonical_path_filters(&scope.path_filters) == canonical_path_filters(path_filters)
        && canonical_filter_values(&scope.language_filters)
            == canonical_filter_values(language_filters)
}

fn canonical_path_filters(filters: &[String]) -> Vec<String> {
    let normalized = filters.iter().map(|filter| {
        let mut value = filter.trim_end_matches(['/', '\\']);
        while let Some(stripped) = value.strip_prefix("./") {
            value = stripped;
        }
        value.to_owned()
    });
    canonical_filter_values_from_iter(normalized)
}

fn canonical_filter_values(filters: &[String]) -> Vec<String> {
    canonical_filter_values_from_iter(filters.iter().cloned())
}

fn canonical_filter_values_from_iter(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut canonical = Vec::new();
    for value in values {
        if !canonical.contains(&value) {
            canonical.push(value);
        }
    }
    canonical
}

async fn active_index_task_for_target(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
    target: &FreshFullIndexProbe,
) -> Result<Option<CodeIndexTaskRecord>, ApiError> {
    let Some(active_task) = store
        .active_code_index_task(status.repository_id.clone())
        .await
        .map_err(storage_api_error)?
    else {
        return Ok(None);
    };
    if !active_task.state.is_unfinished()
        || active_task.resolved_commit_sha != target.resolved_commit_sha
    {
        return Ok(None);
    }
    let Ok(active_request) = serde_json::from_str::<CodeIndexRequest>(&active_task.payload_json)
    else {
        return Ok(None);
    };
    let same_scope_request = active_request.repository.repository == request.repository.repository
        && canonical_path_filters(&active_task.path_filters)
            == canonical_path_filters(&target.path_filters)
        && canonical_filter_values(&active_task.language_filters)
            == canonical_filter_values(&target.language_filters)
        && active_request.workspace_detection == request.workspace_detection;

    Ok(same_scope_request.then_some(active_task))
}

pub(super) async fn active_full_index_task_for_request(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
    payload_json: &str,
) -> Result<Option<CodeIndexTaskRecord>, ApiError> {
    let Some(active_task) = store
        .active_code_index_task(status.repository_id.clone())
        .await
        .map_err(storage_api_error)?
    else {
        return Ok(None);
    };
    if !active_task.state.is_unfinished()
        || active_task.mode != CodeIndexMode::Full
        || active_task.payload_json != payload_json
    {
        return Ok(None);
    }
    let resolved = resolve_code_ref_for_selector(
        status,
        &request.repository,
        request.repository.ref_selector.clone(),
    )
    .await?;
    if resolved == active_task.resolved_commit_sha {
        Ok(Some(active_task))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
