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
const TERMINAL_FIXED_CONTROL_ROWS: usize = 8;

pub(super) fn encoded_summary(
    snapshot: &CodeIndexSnapshot,
    task_id: &str,
    batch_count: usize,
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
        .and_then(|count| count.checked_add(snapshot.framework_nodes.len()))
        .and_then(|count| count.checked_add(snapshot.framework_edges.len()))
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
        batch_count,
    };
    let encoded = super::super::checkpoint_receipt::encode(&receipt)?;
    Ok((receipt, encoded))
}

pub(super) fn begin_batched_delta(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    completion: &CloneCompletion,
) -> Result<usize, StorageError> {
    let file_count = completion
        .cloned_file_count
        .checked_add(snapshot.files.len())
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    let base_batch_count = usize::from(completion.cloned_file_count > 0);
    let last_path = transaction
        .query_row(
            "SELECT path FROM code_repository_files
             WHERE source_scope = ?1 ORDER BY path DESC LIMIT 1",
            [&snapshot.source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if last_path.is_some() != (completion.cloned_file_count > 0) {
        return Err(StorageError::Invariant(format!(
            "incremental clone base prefix for scope '{}' is inconsistent",
            snapshot.source_scope
        )));
    }
    let base_fact_rows = if completion.cloned_file_count == 0 {
        0
    } else {
        completion.base_source_fact_row_upper_bound
    };
    let changed = transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET state = ?3, total_path_count = ?4,
             parsed_file_count = ?5, committed_file_count = ?5,
             committed_symbol_count = ?6, committed_reference_count = ?7,
             committed_chunk_count = ?8, committed_fact_row_count = ?9,
             incremental_summary_json = NULL, batch_count = ?10,
             last_path = ?11, updated_at_ms = ?12, error_message = NULL
         WHERE source_scope = ?1 AND repository_id = ?2 AND state = ?13
           AND resolved_commit_sha = ?14 AND tree_hash = ?15
           AND path_filters_json = ?16 AND language_filters_json = ?17",
        params![
            snapshot.source_scope,
            snapshot.repository_id,
            FINALIZATION_HANDOFF_STATE,
            file_count,
            completion.cloned_file_count,
            completion.cloned_symbol_count,
            completion.cloned_reference_count,
            completion.cloned_chunk_count,
            base_fact_rows,
            base_batch_count,
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
        return Ok(base_batch_count);
    }
    Err(StorageError::Invariant(format!(
        "incremental clone checkpoint for scope '{}' changed before batched delta startup",
        snapshot.source_scope
    )))
}

pub(super) fn require_terminal_control_budget(
    snapshot: &CodeIndexSnapshot,
    completion: &CloneCompletion,
    delta_batch_count: usize,
    budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    let (_, encoded_summary) =
        encoded_summary(snapshot, &completion.task_id, delta_batch_count.max(1))?;
    let rows = completion
        .terminal_cleanup_rows
        .checked_add(snapshot.tombstones.len())
        .and_then(|count| count.checked_add(TERMINAL_FIXED_CONTROL_ROWS))
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    let fixed_control_bytes = admission_control_bytes(snapshot)?
        .checked_mul(TERMINAL_FIXED_CONTROL_ROWS)
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    let mut bytes = completion
        .terminal_cleanup_bytes
        .checked_add(encoded_summary.len())
        .and_then(|count| count.checked_add(fixed_control_bytes))
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    for tombstone in &snapshot.tombstones {
        bytes = bytes
            .checked_add(super::admission::ROW_STORAGE_OVERHEAD_BYTES)
            .and_then(|count| count.checked_add(tombstone.repository_id.len()))
            .and_then(|count| count.checked_add(tombstone.source_scope.len()))
            .and_then(|count| count.checked_add(tombstone.old_path.len()))
            .and_then(|count| {
                count.checked_add(tombstone.new_path.as_deref().unwrap_or_default().len())
            })
            .and_then(|count| count.checked_add(tombstone.base_ref.len()))
            .and_then(|count| count.checked_add(tombstone.head_ref.len()))
            .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    }
    if rows > budget.max_rows_per_batch || bytes > budget.max_bytes_per_batch {
        return Err(StorageError::CapacityExceeded(format!(
            "incremental clone terminal control surface for scope '{}' exceeds its durable row or byte budget",
            snapshot.source_scope
        )));
    }
    Ok(())
}

pub(super) fn mark_batched_delta_ready_for_finalization(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
    completion: &CloneCompletion,
    delta_batch_count: usize,
) -> Result<(), StorageError> {
    let expected_files = checked_count(
        completion.cloned_file_count,
        snapshot.files.len(),
        &snapshot.source_scope,
    )?;
    let expected_symbols = checked_count(
        completion.cloned_symbol_count,
        snapshot.symbols.len(),
        &snapshot.source_scope,
    )?;
    let expected_references = checked_count(
        completion.cloned_reference_count,
        snapshot.references.len(),
        &snapshot.source_scope,
    )?;
    let expected_chunks = checked_count(
        completion.cloned_chunk_count,
        snapshot.chunks.len(),
        &snapshot.source_scope,
    )?;
    let expected_batches = usize::from(completion.cloned_file_count > 0)
        .checked_add(delta_batch_count)
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))?;
    let receipt_batches = delta_batch_count.max(1);
    let (_, encoded_summary) = encoded_summary(snapshot, &completion.task_id, receipt_batches)?;
    let last_path = transaction
        .query_row(
            "SELECT path FROM code_repository_files
             WHERE source_scope = ?1 ORDER BY path DESC LIMIT 1",
            [&snapshot.source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if last_path.is_some() != (expected_files > 0) {
        return Err(StorageError::Invariant(format!(
            "incremental clone final file prefix for scope '{}' is inconsistent",
            snapshot.source_scope
        )));
    }
    insert_tombstones(transaction, snapshot)?;
    let changed = transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET incremental_summary_json = ?8, updated_at_ms = ?9,
             last_path = ?10, error_message = NULL
         WHERE source_scope = ?1 AND repository_id = ?2 AND state = ?11
           AND parsed_file_count = ?3 AND committed_file_count = ?3
           AND committed_symbol_count = ?4 AND committed_reference_count = ?5
           AND committed_chunk_count = ?6 AND batch_count = ?7
           AND incremental_summary_json IS NULL",
        params![
            snapshot.source_scope,
            snapshot.repository_id,
            expected_files,
            expected_symbols,
            expected_references,
            expected_chunks,
            expected_batches,
            encoded_summary,
            now_millis()?,
            last_path,
            FINALIZATION_HANDOFF_STATE,
        ],
    )?;
    if changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone checkpoint for scope '{}' changed before batched delta handoff",
        snapshot.source_scope
    )))
}

fn insert_tombstones(
    transaction: &Transaction<'_>,
    snapshot: &CodeIndexSnapshot,
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "INSERT OR REPLACE INTO code_repository_path_tombstones
            (repository_id, source_scope, old_path, new_path, base_ref, head_ref)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for tombstone in &snapshot.tombstones {
        statement.execute(params![
            tombstone.repository_id,
            tombstone.source_scope,
            tombstone.old_path,
            tombstone.new_path,
            tombstone.base_ref,
            tombstone.head_ref,
        ])?;
    }
    Ok(())
}

fn admission_control_bytes(snapshot: &CodeIndexSnapshot) -> Result<usize, StorageError> {
    super::admission::ROW_STORAGE_OVERHEAD_BYTES
        .checked_add(snapshot.repository_id.len())
        .and_then(|count| count.checked_add(snapshot.source_scope.len()))
        .ok_or_else(|| handoff_capacity_error(&snapshot.source_scope))
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

#[cfg(test)]
#[path = "durable_handoff_tests.rs"]
mod tests;
