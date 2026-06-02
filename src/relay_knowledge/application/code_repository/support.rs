use std::path::PathBuf;

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse},
    code::{
        CodeIndexError, SOURCE_GREP_CANDIDATE_FILE_LIMIT, prepare_full_index_plan,
        repository_uses_filesystem_source, resolve_repository_ref_with_filters,
        resolve_repository_snapshot_with_filters, source_declarations_for_identity,
        source_grep_matches,
    },
    domain::{
        CodeFeatureFlagRequest, CodeIndexCheckpoint, CodeIndexMode, CodeIndexRequest,
        CodeIndexResourceBudget, CodeIndexTaskRecord, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRepositoryStatus, CodeRetrievalRequest,
        code_snapshot_expected_scope_id,
    },
    storage::{
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE, CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
        StorageError,
    },
};

use super::source_fallback::{
    append_code_grep_fallback, append_definition_source_fallback, plan_code_grep_fallback,
};

pub(super) const CODE_INDEX_TASK_LEASE_MS: u64 = 30 * 60 * 1000;
pub(super) const CODE_INDEX_TASK_MAX_ATTEMPTS: u32 = 3;
pub(super) const CODE_INDEX_TASK_RETRY_BACKOFF_MS: u64 = 60_000;
pub(super) const RETAIN_RECENT_CODE_SCOPES: usize = 2;
pub(super) const CODE_INDEX_WORKER_LEASE_OWNER_PREFIX: &str = "code-index-worker-";

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

pub(super) async fn code_status_checkpoint(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &CodeRepositoryStatus,
    active_task: Option<&CodeIndexTaskRecord>,
) -> Result<Option<CodeIndexCheckpoint>, ApiError> {
    if let Some(task) = active_task {
        return store
            .code_index_checkpoint(task.source_scope.clone())
            .await
            .map_err(storage_api_error);
    }
    if status.state == "indexing"
        && let Some(checkpoint) = store
            .latest_code_index_checkpoint(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?
    {
        return Ok(Some(checkpoint));
    }
    if let Some(scope) = status.last_indexed_scope_id.clone()
        && let Some(checkpoint) = store
            .code_index_checkpoint(scope)
            .await
            .map_err(storage_api_error)?
    {
        return Ok(Some(checkpoint));
    }

    Ok(None)
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

pub(super) async fn recover_orphaned_code_index_task_leases(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    now_ms: u64,
) -> Result<usize, ApiError> {
    recover_code_index_task_leases(store, now_ms).await?;
    let running_leases = store
        .running_code_index_task_leases()
        .await
        .map_err(storage_api_error)?;
    if running_leases.is_empty() {
        return Ok(0);
    }
    let orphaned_task_ids = tokio::task::spawn_blocking(move || {
        running_leases
            .into_iter()
            .filter_map(|lease| {
                let pid = code_index_worker_pid(&lease.lease_owner)?;
                (!process_is_running(pid)).then_some(lease.task_id)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
    if orphaned_task_ids.is_empty() {
        return Ok(0);
    }

    match store
        .recover_code_index_task_leases_by_task(crate::storage::CodeIndexTaskLeaseRecovery {
            task_ids: orphaned_task_ids,
            now_ms,
            max_attempts: CODE_INDEX_TASK_MAX_ATTEMPTS,
            error_kind: "lease_orphaned".to_owned(),
            error_message: "code index task lease owner process is not running".to_owned(),
        })
        .await
    {
        Ok(recovered) => Ok(recovered),
        Err(error)
            if storage_error_message_is(&error, CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE) =>
        {
            Ok(0)
        }
        Err(error) => Err(storage_api_error(error)),
    }
}

pub(super) fn code_index_worker_lease_owner() -> String {
    format!(
        "{CODE_INDEX_WORKER_LEASE_OWNER_PREFIX}{}",
        std::process::id()
    )
}

fn storage_error_message_is(error: &StorageError, expected: &str) -> bool {
    matches!(error, StorageError::InvalidInput(message) if message == expected)
}

fn code_index_worker_pid(lease_owner: &str) -> Option<u32> {
    let suffix = lease_owner.strip_prefix(CODE_INDEX_WORKER_LEASE_OWNER_PREFIX)?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    suffix.parse::<u32>().ok()
}

fn process_is_running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }

    process_is_running_by_platform(pid)
}

#[cfg(windows)]
fn process_is_running_by_platform(pid: u32) -> bool {
    let needle = format!(",\"{pid}\",");
    std::process::Command::new(windows_tasklist_command())
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(&needle))
        .unwrap_or(true)
}

#[cfg(windows)]
fn windows_tasklist_command() -> std::path::PathBuf {
    std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .map(|root| root.join("System32").join("tasklist.exe"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("tasklist.exe"))
}

#[cfg(unix)]
fn process_is_running_by_platform(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .ok()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .any(|value| value == pid.to_string())
        })
        .unwrap_or(true)
}

#[cfg(not(any(unix, windows)))]
fn process_is_running_by_platform(_pid: u32) -> bool {
    true
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
    let CodeIndexMode::Incremental { base_ref, .. } = &request.mode else {
        let fingerprints = store
            .code_file_fingerprints(status.repository_id.clone())
            .await
            .map_err(storage_api_error)?;
        return Ok(PreviousIndexState {
            fingerprints,
            base_resolved_commit_sha: status.last_indexed_commit.clone(),
        });
    };
    let base_commit =
        resolve_code_ref_for_selector(status, &request.repository, base_ref.clone()).await?;
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
    let source_scope = base_scope.last_indexed_scope_id.clone().ok_or_else(|| {
        ApiError::invalid_argument(format!(
            "incremental base ref '{}' has no persisted source scope",
            base_ref
        ))
    })?;
    if !code_scope_matches_current_fact_version(&base_scope) {
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
        let path_filters = plan.path_filters.clone();
        let language_filters = plan.language_filters.clone();
        let identity = identity.clone();
        let declarations = run_blocking_code(move || {
            source_declarations_for_identity(
                &registration,
                &commit,
                paths,
                &path_filters,
                &language_filters,
                &identity,
            )
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
    if !is_generated_git_snapshot_scope(source_scope) {
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

fn is_generated_git_snapshot_scope(source_scope: &str) -> bool {
    let Some(hash) = source_scope.strip_prefix("git_snapshot:") else {
        return false;
    };
    hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
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

pub(super) fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
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
    fn code_index_worker_pid_parses_only_owned_worker_leases() {
        assert_eq!(code_index_worker_pid("code-index-worker-123"), Some(123));
        assert_eq!(code_index_worker_pid("worker-123"), None);
        assert_eq!(code_index_worker_pid("code-index-worker-"), None);
        assert_eq!(code_index_worker_pid("code-index-worker-pid"), None);
    }

    #[test]
    fn current_process_is_treated_as_running() {
        assert!(process_is_running(std::process::id()));
    }

    #[test]
    fn current_fact_version_scope_requires_expected_source_scope() {
        let expected_scope =
            code_snapshot_expected_scope_id("repo", "tree-a", &["src".to_owned()], &[])
                .expect("code snapshots should have a fact-version scope");
        let compatible = status_for_scope(
            Some(expected_scope),
            Some("tree-a"),
            vec!["src".to_owned()],
            Vec::new(),
        );
        let custom_scope = status_for_scope(
            Some("git_snapshot:legacy".to_owned()),
            Some("tree-a"),
            vec!["src".to_owned()],
            Vec::new(),
        );
        let legacy = status_for_scope(
            Some("git_snapshot:0000000000000000".to_owned()),
            Some("tree-a"),
            vec!["src".to_owned()],
            Vec::new(),
        );
        let missing_scope =
            status_for_scope(None, Some("tree-a"), vec!["src".to_owned()], Vec::new());
        let missing_tree = status_for_scope(
            Some("git_snapshot:0000000000000000".to_owned()),
            None,
            vec!["src".to_owned()],
            Vec::new(),
        );

        assert!(code_scope_matches_current_fact_version(&compatible));
        assert!(code_scope_matches_current_fact_version(&custom_scope));
        assert!(!code_scope_matches_current_fact_version(&legacy));
        assert!(!code_scope_matches_current_fact_version(&missing_scope));
        assert!(!code_scope_matches_current_fact_version(&missing_tree));
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

    fn status_for_scope(
        source_scope: Option<String>,
        tree_hash: Option<&str>,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> CodeRepositoryStatus {
        CodeRepositoryStatus {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            root_path: "/tmp/repo".to_owned(),
            path_filters,
            language_filters,
            last_indexed_scope_id: source_scope,
            last_indexed_commit: Some("commit".to_owned()),
            tree_hash: tree_hash.map(str::to_owned),
            state: "indexed".to_owned(),
            indexed_file_count: 1,
            symbol_count: 0,
            reference_count: 0,
            chunk_count: 0,
            stale: false,
            degraded_reason: None,
        }
    }
}
