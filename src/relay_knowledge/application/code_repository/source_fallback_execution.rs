use std::collections::BTreeMap;

use crate::{
    api::ApiError,
    code::{
        SOURCE_GREP_CANDIDATE_FILE_LIMIT, source_declarations_for_identity,
        source_declarations_for_identity_from_worktree_overlay, source_grep_matches,
        source_grep_matches_from_worktree_overlay,
    },
    domain::{CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest},
    storage::{KnowledgeStore, StorageError},
};

use super::{
    source_fallback::{
        append_code_grep_fallback, append_definition_source_fallback, plan_code_grep_fallback,
    },
    support::{registration_from_status, run_blocking_code},
};

pub(super) async fn apply_code_grep_fallback(
    store: &std::sync::Arc<dyn KnowledgeStore>,
    base_status: &CodeRepositoryStatus,
    scoped_status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    results: &mut Vec<CodeRetrievalHit>,
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
                plan.exclude_generated,
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
    let worktree_expected_hashes = if plan.read_worktree_overlay {
        match worktree_overlay_fallback_hashes(store, scoped_status, &plan.paths).await {
            Ok(hashes) => Some(hashes),
            Err(error) => {
                return Ok(Some(format!(
                    "source fallback worktree hash lookup unavailable: {error}"
                )));
            }
        }
    } else {
        None
    };
    let registration = registration_from_status(base_status);
    let commit = plan.commit.clone();
    let source_request = plan.source_request();
    let outcome = if let Some(expected_hashes) = worktree_expected_hashes.clone() {
        run_blocking_code(move || {
            source_grep_matches_from_worktree_overlay(
                &registration,
                expected_hashes,
                source_request,
            )
        })
        .await?
    } else {
        run_blocking_code(move || source_grep_matches(&registration, &commit, source_request))
            .await?
    };
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
        let exclude_generated = plan.exclude_generated;
        let declarations = if let Some(expected_hashes) = worktree_expected_hashes {
            run_blocking_code(move || {
                source_declarations_for_identity_from_worktree_overlay(
                    &registration,
                    expected_hashes,
                    paths,
                    &identity,
                    exclude_generated,
                )
            })
            .await?
        } else {
            run_blocking_code(move || {
                source_declarations_for_identity(
                    &registration,
                    &commit,
                    paths,
                    &path_filters,
                    &language_filters,
                    &identity,
                    exclude_generated,
                )
            })
            .await?
        };
        append_definition_source_fallback(scoped_status, request, results, declarations);
    }

    Ok(fallback_degraded_reason)
}

async fn worktree_overlay_fallback_hashes(
    store: &std::sync::Arc<dyn KnowledgeStore>,
    status: &CodeRepositoryStatus,
    paths: &[String],
) -> Result<BTreeMap<String, String>, StorageError> {
    let source_scope = status.last_indexed_scope_id.as_deref().ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' does not have an indexed source scope",
            status.alias
        ))
    })?;
    let mut selected_paths = paths.to_vec();
    selected_paths.sort();
    selected_paths.dedup();
    let hashes = store
        .code_file_fingerprints_for_paths(source_scope.to_owned(), selected_paths)
        .await?;

    Ok(hashes
        .into_iter()
        .map(|fingerprint| (fingerprint.path, fingerprint.blob_hash))
        .collect())
}
