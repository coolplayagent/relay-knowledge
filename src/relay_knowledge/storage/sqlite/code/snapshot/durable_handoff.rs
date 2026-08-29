//! Owns the atomic durable-clone delta-to-finalization handoff.

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    clock::system_now_millis,
    domain::{CodeIncrementalSummaryReceipt, CodeIndexResourceBudget, CodeIndexSnapshot},
    storage::StorageError,
};

use super::durable_clone::CloneCompletion;

pub(super) const FINALIZATION_HANDOFF_STATE: &str = "indexing";

pub(super) fn encoded_summary(
    snapshot: &CodeIndexSnapshot,
    task_id: &str,
) -> Result<(CodeIncrementalSummaryReceipt, String), StorageError> {
    let base_resolved_commit_sha = snapshot.base_resolved_commit_sha.clone().ok_or_else(|| {
        StorageError::Invariant(format!(
            "durable incremental scope '{}' has no base commit for its summary receipt",
            snapshot.source_scope
        ))
    })?;
    let sqlite_write_count = snapshot
        .files
        .len()
        .checked_add(snapshot.symbols.len())
        .and_then(|count| count.checked_add(snapshot.references.len()))
        .and_then(|count| count.checked_add(snapshot.imports.len()))
        .and_then(|count| count.checked_add(snapshot.dependencies.len()))
        .and_then(|count| count.checked_add(snapshot.calls.len()))
        .and_then(|count| count.checked_add(snapshot.feature_flags.len()))
        .and_then(|count| count.checked_add(snapshot.routes.len()))
        .and_then(|count| count.checked_add(snapshot.chunks.len()))
        .and_then(|count| count.checked_add(snapshot.diagnostics.len()))
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    let receipt = CodeIncrementalSummaryReceipt {
        task_id: task_id.to_owned(),
        base_resolved_commit_sha,
        changed_path_count: snapshot.changed_path_count,
        skipped_unchanged_count: snapshot.skipped_unchanged_count,
        deleted_path_count: snapshot.deleted_paths.len(),
        affected_path_count: snapshot
            .files
            .iter()
            .map(|file| file.path.as_str())
            .chain(snapshot.deleted_paths.iter().map(String::as_str))
            .collect::<BTreeSet<_>>()
            .len(),
        blob_read_count: snapshot.files.len(),
        parsed_file_count: snapshot.files.len(),
        sqlite_write_count,
        degraded_file_count: snapshot.diagnostics.len(),
        batch_count: 1,
    };
    let encoded = super::super::checkpoint_receipt::encode(&receipt)?;
    Ok((receipt, encoded))
}

pub(super) fn mark_delta_ready_for_finalization(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    completion: &CloneCompletion,
    budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    let file_count = checked_count(
        completion.cloned_file_count,
        snapshot.files.len(),
        &snapshot.source_scope,
    )?;
    let symbol_count = checked_count(
        completion.cloned_symbol_count,
        snapshot.symbols.len(),
        &snapshot.source_scope,
    )?;
    let reference_count = checked_count(
        completion.cloned_reference_count,
        snapshot.references.len(),
        &snapshot.source_scope,
    )?;
    let chunk_count = checked_count(
        completion.cloned_chunk_count,
        snapshot.chunks.len(),
        &snapshot.source_scope,
    )?;
    let last_path = transaction
        .query_row(
            "SELECT path FROM code_repository_files
             WHERE source_scope = ?1 ORDER BY path DESC LIMIT 1",
            [&snapshot.source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if last_path.is_some() != (file_count > 0) {
        return Err(StorageError::Invariant(format!(
            "incremental clone file-prefix proof for scope '{}' is inconsistent",
            snapshot.source_scope
        )));
    }
    let delta_fact_rows = snapshot
        .files
        .len()
        .checked_add(snapshot.symbols.len())
        .and_then(|count| count.checked_add(snapshot.references.len()))
        .and_then(|count| count.checked_add(snapshot.imports.len()))
        .and_then(|count| count.checked_add(snapshot.dependencies.len()))
        .and_then(|count| count.checked_add(snapshot.calls.len()))
        .and_then(|count| count.checked_add(snapshot.feature_flags.len()))
        .and_then(|count| count.checked_add(snapshot.routes.len()))
        .and_then(|count| count.checked_add(snapshot.chunks.len()))
        .and_then(|count| count.checked_add(snapshot.diagnostics.len()))
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    let final_fact_row_upper_bound = completion
        .base_source_fact_row_upper_bound
        .checked_add(delta_fact_rows)
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    let proven_batches = final_fact_row_upper_bound
        .checked_add(budget.max_rows_per_batch.saturating_sub(1))
        .map(|rows| rows / budget.max_rows_per_batch)
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    let batch_count = if file_count == 0 {
        0
    } else {
        // The inherited absolute fact bound may span more batches than this delta. Capping at the
        // exact file count preserves the checkpoint prefix invariant without weakening the bound.
        proven_batches.max(1).min(file_count)
    };
    let (_, incremental_summary_json) = encoded_summary(snapshot, &completion.task_id)?;
    let changed = transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET state = ?3, total_path_count = ?4, parsed_file_count = ?4,
             committed_file_count = ?4, committed_symbol_count = ?5,
             committed_reference_count = ?6, committed_chunk_count = ?7,
             batch_count = ?8, committed_fact_row_count = ?9,
             incremental_summary_json = ?10, last_path = ?11, updated_at_ms = ?12,
             error_message = NULL
         WHERE source_scope = ?1 AND repository_id = ?2 AND state = ?13
           AND resolved_commit_sha = ?14 AND tree_hash = ?15
           AND path_filters_json = ?16 AND language_filters_json = ?17",
        params![
            snapshot.source_scope,
            snapshot.repository_id,
            FINALIZATION_HANDOFF_STATE,
            file_count,
            symbol_count,
            reference_count,
            chunk_count,
            batch_count,
            final_fact_row_upper_bound,
            incremental_summary_json,
            last_path,
            now_millis()?,
            completion.checkpoint_state,
            snapshot.resolved_commit_sha,
            snapshot.tree_hash,
            serde_json::to_string(&snapshot.path_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            serde_json::to_string(&snapshot.language_filters)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
        ],
    )?;
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone checkpoint for scope '{}' changed before delta handoff",
        snapshot.source_scope
    )))
}

fn checked_count(left: usize, right: usize, source_scope: &str) -> Result<usize, StorageError> {
    left.checked_add(right)
        .ok_or_else(|| handoff_capacity_error(source_scope))
}

fn handoff_capacity_error(source_scope: &str) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "incremental clone handoff for scope '{source_scope}' exceeds platform capacity"
    ))
}

fn now_millis() -> Result<u64, StorageError> {
    system_now_millis().map_err(|error| {
        StorageError::Invariant(format!("incremental handoff clock is invalid: {error}"))
    })
}
