//! Creates the unpublished clone owner and its first durable checkpoint.

use rusqlite::{Transaction, params};

use crate::{
    domain::{
        CodeIncrementalClonePhase, CodeIndexResourceBudget, CodeIndexSnapshot,
        code_incremental_clone_state,
    },
    storage::StorageError,
};

use super::{
    CloneBaseHeader, CloneIdentity, admission, clone_capacity_error, durable_page_byte_limit,
    now_millis, progress,
};
use crate::storage::sqlite::code::lifecycle::publication_fence::PublicationFenceGuard;

pub(super) fn require_init_budget(
    identity: &CloneIdentity,
    base_scope: &str,
    task_id: &str,
    budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    let rows = identity
        .affected_paths
        .len()
        .checked_add(3)
        .ok_or_else(|| clone_capacity_error(&identity.source_scope))?;
    let checkpoint_state =
        code_incremental_clone_state(CodeIncrementalClonePhase::Tables, 0, 0, 0, "none")
            .ok_or_else(|| clone_capacity_error(&identity.source_scope))?;
    let resource_budget_json = serde_json::to_string(&budget)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let scope_bytes = [
        identity.source_scope.as_str(),
        identity.repository_id.as_str(),
        identity.resolved_commit_sha.as_str(),
        identity.tree_hash.as_str(),
        identity.path_filters_json.as_str(),
        identity.language_filters_json.as_str(),
    ]
    .iter()
    .try_fold(admission::ROW_STORAGE_OVERHEAD_BYTES, |total, value| {
        total
            .checked_add(value.len())
            .ok_or_else(|| clone_capacity_error(&identity.source_scope))
    })?;
    let checkpoint_bytes = [
        identity.source_scope.as_str(),
        identity.repository_id.as_str(),
        checkpoint_state.as_str(),
        identity.resolved_commit_sha.as_str(),
        identity.tree_hash.as_str(),
        identity.path_filters_json.as_str(),
        identity.language_filters_json.as_str(),
        resource_budget_json.as_str(),
    ]
    .iter()
    .try_fold(
        admission::ROW_STORAGE_OVERHEAD_BYTES + 10 * 8,
        |total, value| {
            total
                .checked_add(value.len())
                .ok_or_else(|| clone_capacity_error(&identity.source_scope))
        },
    )?;
    let progress_bytes = [
        identity.source_scope.as_str(),
        identity.repository_id.as_str(),
        base_scope,
        task_id,
        identity.delta_digest.as_str(),
        progress::PHASE_TABLES,
    ]
    .iter()
    .try_fold(
        admission::ROW_STORAGE_OVERHEAD_BYTES + 27 * 8 + 9,
        |total, value| {
            total
                .checked_add(value.len())
                .ok_or_else(|| clone_capacity_error(&identity.source_scope))
        },
    )?;
    let mut bytes = scope_bytes
        .checked_add(checkpoint_bytes)
        .and_then(|value| value.checked_add(progress_bytes))
        .ok_or_else(|| clone_capacity_error(&identity.source_scope))?;
    for path in &identity.affected_paths {
        bytes = bytes
            .checked_add(identity.source_scope.len())
            .and_then(|value| value.checked_add(path.len()))
            .and_then(|value| value.checked_add(admission::ROW_STORAGE_OVERHEAD_BYTES))
            .ok_or_else(|| clone_capacity_error(&identity.source_scope))?;
    }
    if rows > budget.max_rows_per_batch || bytes > budget.max_bytes_per_batch {
        return Err(clone_capacity_error(&identity.source_scope));
    }
    Ok(())
}

pub(super) fn initialize_progress(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    identity: &CloneIdentity,
    base: &CloneBaseHeader<'_>,
    guard: &PublicationFenceGuard,
    budget: CodeIndexResourceBudget,
) -> Result<progress::CloneProgress, StorageError> {
    admission::require_unused_target(transaction, &identity.source_scope)?;
    stage_empty_target(transaction, identity)?;
    let progress = progress::CloneProgress {
        source_scope: identity.source_scope.clone(),
        repository_id: identity.repository_id.clone(),
        base_scope: base.source_scope.to_owned(),
        task_id: guard.task_id().to_owned(),
        delta_digest: identity.delta_digest.clone(),
        phase: progress::PHASE_TABLES.to_owned(),
        table_ordinal: 0,
        completed_page_ordinal: 0,
        cursor_key: None,
        cursor_tiebreaker: None,
        completed_table_ordinal: None,
        expected_table_rows: None,
        scanned_table_rows: 0,
        copied_table_rows: 0,
        scanned_total_rows: 0,
        copied_total_rows: 0,
        copied_total_bytes: 0,
        cloned_file_count: 0,
        cloned_symbol_count: 0,
        cloned_reference_count: 0,
        cloned_chunk_count: 0,
        cloned_diagnostic_count: 0,
        cloned_reference_group_count: 0,
        cloned_search_document_count: 0,
        base_manifest_reference_count: base.manifest_reference_count,
        base_manifest_group_count: base.manifest_group_count,
        scanned_reference_occurrence_count: 0,
        scanned_reference_row_count: 0,
        scanned_reference_group_count: 0,
        scanned_reference_search_owner_count: 0,
        base_source_fact_row_upper_bound: base.source_fact_row_upper_bound,
        page_row_limit: budget.max_rows_per_batch,
        page_byte_limit: durable_page_byte_limit(budget),
    };
    let now_ms = now_millis()?;
    let checkpoint_state = progress::checkpoint_state(&progress)?;
    transaction.execute(
        "INSERT INTO code_repository_index_checkpoints (
             source_scope, repository_id, state, resolved_commit_sha, tree_hash,
             path_filters_json, language_filters_json, total_path_count,
             parsed_file_count, committed_file_count, committed_symbol_count,
             committed_reference_count, committed_chunk_count, batch_count, last_path,
             resource_budget_json, updated_at_ms, error_message
         ) VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
             ?8, 0, 0, 0, 0, 0, NULL, ?9, ?10, NULL
         )",
        params![
            identity.source_scope,
            identity.repository_id,
            checkpoint_state,
            identity.resolved_commit_sha,
            identity.tree_hash,
            identity.path_filters_json,
            identity.language_filters_json,
            snapshot.files.len(),
            serde_json::to_string(&budget)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            now_ms,
        ],
    )?;
    progress::insert(transaction, &progress, now_ms)?;
    let mut insert_path = transaction.prepare_cached(
        "INSERT INTO code_repository_incremental_clone_affected_paths (source_scope, path)
         VALUES (?1, ?2)",
    )?;
    for path in &identity.affected_paths {
        insert_path.execute(params![identity.source_scope, path])?;
    }
    Ok(progress)
}

fn stage_empty_target(
    transaction: &Transaction<'_>,
    identity: &CloneIdentity,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO code_repository_scopes (
             source_scope, repository_id, resolved_commit_sha, tree_hash,
             path_filters_json, language_filters_json, indexed_file_count,
             symbol_count, reference_count, chunk_count, stale, degraded_reason
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0, 0, 1, NULL)",
        params![
            identity.source_scope,
            identity.repository_id,
            identity.resolved_commit_sha,
            identity.tree_hash,
            identity.path_filters_json,
            identity.language_filters_json,
        ],
    )?;
    Ok(())
}
