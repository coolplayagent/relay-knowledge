//! Owns persisted index-state inspection and index reuse decisions.

use std::path::PathBuf;

use crate::{
    api::{ApiError, CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse},
    code::{
        prepare_full_index_plan, repository_uses_filesystem_source,
        resolve_repository_snapshot_with_filters,
    },
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget, CodeIndexTaskRecord,
        CodeRepositorySelector, CodeRepositoryStatus,
    },
};

use super::super::{
    blocking::run_blocking_code,
    errors::storage_api_error,
    repository_status::registration_from_status,
    scope::{
        code_scope_matches_current_fact_version, merged_filters, resolve_code_ref_for_selector,
    },
};

pub(super) const RETAIN_RECENT_CODE_SCOPES: usize = 2;

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
mod tests {
    use super::*;

    #[test]
    fn degraded_file_count_uses_index_status_reason_shape() {
        let status = CodeRepositoryStatus {
            degraded_reason: Some("25 file(s) degraded during code indexing".to_owned()),
            ..status_for_scope()
        };
        let custom = CodeRepositoryStatus {
            degraded_reason: Some("custom parser warning".to_owned()),
            ..status.clone()
        };

        assert_eq!(degraded_file_count_from_status(&status), Some(25));
        assert_eq!(degraded_file_count_from_status(&custom), None);
    }

    fn status_for_scope() -> CodeRepositoryStatus {
        CodeRepositoryStatus {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            root_path: "/tmp/repo".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            last_indexed_scope_id: None,
            last_indexed_commit: Some("commit".to_owned()),
            tree_hash: Some("tree-a".to_owned()),
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
