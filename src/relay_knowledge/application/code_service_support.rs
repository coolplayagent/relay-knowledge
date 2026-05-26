use std::path::PathBuf;

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse},
    code::{
        CodeIndexError, SOURCE_GREP_CANDIDATE_FILE_LIMIT, resolve_repository_ref,
        source_declarations_for_identity, source_grep_matches,
    },
    domain::{
        CodeFeatureFlagRequest, CodeIndexMode, CodeIndexRequest, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalRequest,
    },
    storage::{
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE, CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
        StorageError,
    },
};

use super::code_query_source_fallback::{
    append_code_grep_fallback, append_definition_source_fallback, plan_code_grep_fallback,
};

pub(super) const CODE_INDEX_TASK_LEASE_MS: u64 = 30 * 60 * 1000;
pub(super) const CODE_INDEX_TASK_MAX_ATTEMPTS: u32 = 3;
pub(super) const CODE_INDEX_TASK_RETRY_BACKOFF_MS: u64 = 60_000;
pub(super) const RETAIN_RECENT_CODE_SCOPES: usize = 2;

pub(super) async fn required_code_repository(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    repository: &str,
) -> Result<crate::domain::CodeRepositoryStatus, ApiError> {
    store
        .code_repository_status(repository.to_owned())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!("code repository '{repository}' is not registered"))
        })
}

#[derive(Debug, Clone)]
pub(super) struct CodeIndexTaskLeaseContext {
    pub(super) task_id: String,
    pub(super) lease_owner: String,
    pub(super) attempt_count: u32,
    pub(super) lease_duration_ms: u64,
}

pub(super) async fn refresh_code_index_task_lease(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    lease: Option<&CodeIndexTaskLeaseContext>,
) -> Result<(), ApiError> {
    let Some(lease) = lease else {
        return Ok(());
    };
    let renewal = crate::storage::CodeIndexTaskLeaseRenewal {
        task_id: lease.task_id.clone(),
        lease_owner: lease.lease_owner.clone(),
        attempt_count: lease.attempt_count,
        lease_duration_ms: lease.lease_duration_ms,
        now_ms: now_millis(),
    };
    match store.renew_code_index_task_lease(renewal).await {
        Ok(_) => Ok(()),
        Err(error)
            if storage_error_message_is(&error, CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE) =>
        {
            Ok(())
        }
        Err(error) => Err(storage_api_error(error)),
    }
}

pub(super) async fn recover_code_index_task_leases(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    now_ms: u64,
) -> Result<(), ApiError> {
    match store
        .recover_code_index_task_leases(now_ms, CODE_INDEX_TASK_MAX_ATTEMPTS)
        .await
    {
        Ok(()) => Ok(()),
        Err(error)
            if storage_error_message_is(&error, CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE) =>
        {
            Ok(())
        }
        Err(error) => Err(storage_api_error(error)),
    }
}

fn storage_error_message_is(error: &StorageError, expected: &str) -> bool {
    matches!(error, StorageError::InvalidInput(message) if message == expected)
}

pub(super) fn registration_from_status(
    status: &crate::domain::CodeRepositoryStatus,
) -> CodeRepositoryRegistration {
    CodeRepositoryRegistration {
        repository_id: status.repository_id.clone(),
        alias: status.alias.clone(),
        root_path: status.root_path.clone(),
        path_filters: status.path_filters.clone(),
        language_filters: status.language_filters.clone(),
    }
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

pub(super) async fn previous_fingerprints_for_index(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    request: &CodeIndexRequest,
) -> Result<Vec<crate::domain::CodeFileFingerprint>, ApiError> {
    let CodeIndexMode::Incremental { base_ref, .. } = &request.mode else {
        return store
            .code_file_fingerprints(status.repository_id.clone())
            .await
            .map_err(storage_api_error);
    };
    let base_commit = resolve_code_ref(status, base_ref.clone()).await?;
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
            ApiError::invalid_argument(format!(
                "incremental base ref '{}' resolves to {}, but code repository '{}' has no matching indexed base scope; run repo index --ref {} before repo update",
                base_ref, base_commit, status.alias, base_ref
            ))
        })?;
    if base_scope.stale {
        return Err(ApiError::invalid_argument(format!(
            "incremental base ref '{}' resolves to a stale indexed scope {}; refresh or reindex the base before repo update",
            base_ref,
            base_scope
                .last_indexed_scope_id
                .as_deref()
                .unwrap_or("unscoped")
        )));
    }
    let source_scope = base_scope.last_indexed_scope_id.ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "incremental base ref '{}' has no persisted source scope",
            base_ref
        ))
    })?;

    store
        .code_file_fingerprints_for_scope(source_scope)
        .await
        .map_err(storage_api_error)
}

pub(super) async fn apply_code_grep_fallback(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    base_status: &CodeRepositoryStatus,
    scoped_status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &mut Vec<crate::domain::CodeRetrievalHit>,
) -> Result<Option<String>, ApiError> {
    let Some(plan) = plan_code_grep_fallback(scoped_status, request, results) else {
        return Ok(None);
    };
    let plan = if plan.needs_scope_paths() {
        let source_scope = scoped_status
            .last_indexed_scope_id
            .as_deref()
            .ok_or_else(|| {
                ApiError::invalid_argument(format!(
                    "code repository '{}' does not have an indexed source scope",
                    scoped_status.alias
                ))
            })?;
        let paths = match store
            .code_file_candidate_paths_for_query_scope(
                source_scope.to_owned(),
                plan.query.clone(),
                plan.path_filters.clone(),
                plan.language_filters.clone(),
                SOURCE_GREP_CANDIDATE_FILE_LIMIT.saturating_add(1),
            )
            .await
        {
            Ok(paths) => paths,
            Err(error) => {
                return Ok(Some(format!(
                    "source fallback candidate path lookup unavailable: {error}"
                )));
            }
        };
        plan.with_scope_paths(paths)
    } else {
        plan
    };
    let registration = registration_from_status(base_status);
    let commit = plan.commit.clone();
    let source_request = plan.source_request();
    let outcome =
        run_blocking_code(move || source_grep_matches(&registration, &commit, source_request))
            .await?;
    let had_matches = !outcome.matches.is_empty();
    let fallback_degraded_reason =
        append_code_grep_fallback(scoped_status, request, results, &plan, outcome);
    if !had_matches
        && plan.kind == crate::code::SourceGrepKind::Definition
        && let Some(identity) = &plan.identity
    {
        let registration = registration_from_status(base_status);
        let commit = plan.commit.clone();
        let paths = plan.paths.clone();
        let identity = identity.clone();
        let declarations = run_blocking_code(move || {
            source_declarations_for_identity(&registration, &commit, paths, &identity)
        })
        .await?;
        append_definition_source_fallback(scoped_status, request, results, declarations);
    }

    Ok(fallback_degraded_reason)
}

pub(super) async fn retrieval_request_at_indexed_ref(
    mut request: CodeRetrievalRequest,
    status: &CodeRepositoryStatus,
) -> Result<CodeRetrievalRequest, ApiError> {
    request.repository.ref_selector =
        indexed_commit_for_ref(status, request.repository.ref_selector.clone()).await?;

    Ok(request)
}

pub(super) async fn feature_flag_request_at_indexed_ref(
    mut request: CodeFeatureFlagRequest,
    status: &CodeRepositoryStatus,
) -> Result<CodeFeatureFlagRequest, ApiError> {
    request.repository.ref_selector =
        indexed_commit_for_ref(status, request.repository.ref_selector.clone()).await?;

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
        .map_err(storage_api_error)?;
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
        }
        None => None,
    };
    scoped_status.ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "code repository '{}' has no index for ref {} and requested filters",
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

    Ok(status)
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

    Ok(task.resolved_commit_sha == selector.ref_selector
        && active_languages_cover_requested_scope(
            &status.language_filters,
            &task.language_filters,
            &selector.language_filters,
        )
        && active_paths_cover_requested_scope(
            &status.path_filters,
            &task.path_filters,
            &selector.path_filters,
        ))
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

pub(super) fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

async fn indexed_commit_for_ref(
    status: &CodeRepositoryStatus,
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
        return Err(ApiError::invalid_argument(format!(
            "code repository '{}' has no active worktree overlay",
            status.alias
        )));
    }

    resolve_code_ref(status, ref_selector).await
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

pub(super) async fn resolve_code_ref(
    status: &CodeRepositoryStatus,
    ref_selector: String,
) -> Result<String, ApiError> {
    let root = PathBuf::from(status.root_path.clone());

    run_blocking_code(move || resolve_repository_ref(root, &ref_selector)).await
}

pub(super) async fn run_blocking_code<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CodeIndexError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
        .map_err(code_api_error)
}

fn code_api_error(error: CodeIndexError) -> ApiError {
    match error {
        CodeIndexError::InvalidInput(message) => ApiError::invalid_argument(message),
        CodeIndexError::Git { .. } | CodeIndexError::Io(_) | CodeIndexError::TreeSitter(_) => {
            ApiError::storage_unavailable(error.to_string())
        }
    }
}

pub(super) fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_default_optional_code_index_lease_unavailable_errors() {
        assert!(storage_error_message_is(
            &StorageError::InvalidInput(CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE.to_owned()),
            CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
        ));
        assert!(storage_error_message_is(
            &StorageError::InvalidInput(CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE.to_owned()),
            CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE,
        ));
        assert!(!storage_error_message_is(
            &StorageError::InvalidInput("code index task lease expired".to_owned()),
            CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
        ));
    }

    #[test]
    fn active_path_filters_preserve_registration_scope_boundaries() {
        let registration = vec!["src".to_owned()];
        let narrow_task = vec!["src".to_owned(), "src/a.rs".to_owned()];

        assert!(!active_paths_cover_requested_scope(
            &registration,
            &narrow_task,
            &[]
        ));
        assert!(active_paths_cover_requested_scope(
            &registration,
            &narrow_task,
            &["src/a.rs".to_owned()]
        ));
        assert!(active_paths_cover_requested_scope(
            &registration,
            &registration,
            &["src/a.rs".to_owned()]
        ));
        assert!(!active_paths_cover_requested_scope(
            &registration,
            &registration,
            &["tests/a.rs".to_owned()]
        ));
        assert!(!active_paths_cover_requested_scope(
            &[],
            &["src/a.rs".to_owned()],
            &["src".to_owned()]
        ));
    }

    #[test]
    fn active_language_filters_preserve_registration_scope_boundaries() {
        assert!(!active_languages_cover_requested_scope(
            &[],
            &["python".to_owned()],
            &[]
        ));
        assert!(active_languages_cover_requested_scope(
            &[],
            &["python".to_owned()],
            &["python".to_owned()]
        ));
        assert!(!active_languages_cover_requested_scope(
            &["rust".to_owned()],
            &["rust".to_owned()],
            &["python".to_owned()]
        ));
        assert!(!active_languages_cover_requested_scope(
            &[],
            &["python".to_owned()],
            &["rust".to_owned()]
        ));
    }
}
