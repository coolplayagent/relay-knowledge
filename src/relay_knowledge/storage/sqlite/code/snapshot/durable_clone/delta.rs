//! Validates the clone-complete owner while its dirty delta is committed in durable batches.

use rusqlite::TransactionBehavior;

use super::*;

pub(in crate::storage::sqlite::code::snapshot) fn batched_delta_completion(
    connection: &mut rusqlite::Connection,
    snapshot: &CodeIndexSnapshot,
    guard: &PublicationFenceGuard,
) -> Result<Option<CloneCompletion>, StorageError> {
    guard.validate_repository(&snapshot.repository_id)?;
    if !active_batched_delta_exists(connection, &snapshot.source_scope)? {
        return Ok(None);
    }
    let budget = guard.resource_budget(connection)?;
    let identity = CloneIdentity::from_snapshot(
        snapshot,
        admission::snapshot_delta_digest(snapshot)?,
        budget,
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    guard.validate_target_scope(&transaction, &snapshot.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, &identity)?;
    if guard.resource_budget(&transaction)? != budget {
        return Err(StorageError::Invariant(format!(
            "incremental clone task '{}' changed its resource budget",
            guard.task_id()
        )));
    }
    admission::require_no_workspace_projection(&transaction, snapshot)?;
    let Some(progress) = progress::load(&transaction, &snapshot.source_scope)? else {
        return Ok(None);
    };
    let delta_is_active = transaction.query_row(
        "SELECT state = 'indexing' AND incremental_summary_json IS NULL
         FROM code_repository_index_checkpoints WHERE source_scope = ?1",
        [&snapshot.source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if !delta_is_active {
        return Ok(None);
    }
    validation::validate_delta_progress(&transaction, &progress, &identity, guard, budget)?;
    validate_affected_paths(&transaction, &identity)?;
    let completion = clone_completion(progress, "indexing".to_owned(), &identity.affected_paths)?;
    guard.validate_target_scope(&transaction, &identity.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, &identity)?;
    transaction.commit()?;
    Ok(Some(completion))
}

fn active_batched_delta_exists(
    connection: &rusqlite::Connection,
    source_scope: &str,
) -> Result<bool, StorageError> {
    connection
        .query_row(
            "SELECT EXISTS (
                 SELECT 1
                 FROM code_repository_incremental_clone_progress progress
                 JOIN code_repository_index_checkpoints checkpoint
                   ON checkpoint.source_scope = progress.source_scope
                 WHERE progress.source_scope = ?1
                   AND checkpoint.state = 'indexing'
                   AND checkpoint.incremental_summary_json IS NULL
             )",
            [source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite::code::snapshot) fn start_batched_delta(
    connection: &mut rusqlite::Connection,
    snapshot: &CodeIndexSnapshot,
    session: &CloneSession,
    delta_batch_count: usize,
    guard: &PublicationFenceGuard,
) -> Result<CloneCompletion, StorageError> {
    let budget = guard.resource_budget(connection)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    guard.validate_target_scope(&transaction, &snapshot.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, &session.identity)?;
    let completion =
        validate_clone_complete(&transaction, snapshot, &session.identity, guard, budget)?;
    super::super::durable_handoff::require_terminal_control_budget(
        snapshot,
        &completion,
        delta_batch_count,
        budget,
    )?;
    super::super::durable_handoff::begin_batched_delta(&transaction, snapshot, &completion)?;
    guard.validate_target_scope(&transaction, &snapshot.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, &session.identity)?;
    transaction.commit()?;
    Ok(CloneCompletion {
        checkpoint_state: "indexing".to_owned(),
        ..completion
    })
}

pub(in crate::storage::sqlite::code::snapshot) fn finish_batched_delta(
    connection: &mut rusqlite::Connection,
    snapshot: &CodeIndexSnapshot,
    completion: &CloneCompletion,
    delta_batch_count: usize,
    guard: &PublicationFenceGuard,
) -> Result<(), StorageError> {
    let budget = guard.resource_budget(connection)?;
    let identity = CloneIdentity::from_snapshot(
        snapshot,
        admission::snapshot_delta_digest(snapshot)?,
        budget,
    )?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    guard.validate_target_scope(&transaction, &snapshot.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, &identity)?;
    let progress = progress::load(&transaction, &snapshot.source_scope)?.ok_or_else(|| {
        StorageError::Invariant(format!(
            "incremental clone progress for scope '{}' disappeared before batched delta handoff",
            snapshot.source_scope
        ))
    })?;
    validation::validate_delta_progress(&transaction, &progress, &identity, guard, budget)?;
    super::super::durable_handoff::mark_batched_delta_ready_for_finalization(
        &transaction,
        snapshot,
        completion,
        delta_batch_count,
    )?;
    remove_after_delta(&transaction, &snapshot.source_scope)?;
    guard.validate_target_scope(&transaction, &snapshot.source_scope)?;
    guard.validate(&transaction)?;
    validate_partitioned_target(&transaction, guard, &identity)?;
    transaction.commit()?;
    Ok(())
}

pub(super) fn clone_completion(
    progress: progress::CloneProgress,
    checkpoint_state: String,
    affected_paths: &std::collections::BTreeSet<String>,
) -> Result<CloneCompletion, StorageError> {
    let (terminal_cleanup_rows, terminal_cleanup_bytes) =
        progress::cleanup_surface(&progress, affected_paths)?;
    Ok(CloneCompletion {
        task_id: progress.task_id,
        checkpoint_state,
        cloned_file_count: progress.cloned_file_count,
        cloned_symbol_count: progress.cloned_symbol_count,
        cloned_reference_count: progress.cloned_reference_count,
        cloned_chunk_count: progress.cloned_chunk_count,
        base_source_fact_row_upper_bound: progress.base_source_fact_row_upper_bound,
        completed_page_ordinal: progress.completed_page_ordinal,
        terminal_cleanup_rows,
        terminal_cleanup_bytes,
    })
}
