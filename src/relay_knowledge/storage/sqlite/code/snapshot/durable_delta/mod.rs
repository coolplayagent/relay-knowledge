//! Advances an oversized incremental delta through the existing replay-safe batch protocol.

use rusqlite::OptionalExtension;

use crate::{
    domain::{CodeIndexResourceBudget, CodeIndexSnapshot},
    storage::StorageError,
};

use super::durable_clone;
use crate::storage::sqlite::code::lifecycle::publication_fence::PublicationFenceGuard;

mod batches;

use batches::DeltaBatchPlan;

pub(super) enum DeltaAdvance {
    Pending {
        completed_steps: usize,
        max_steps: usize,
    },
    FinalizationRequired,
}

pub(super) fn resume(
    connection: &mut rusqlite::Connection,
    snapshot: &CodeIndexSnapshot,
    guard: &PublicationFenceGuard,
) -> Result<Option<DeltaAdvance>, StorageError> {
    let Some(completion) = durable_clone::batched_delta_completion(connection, snapshot, guard)?
    else {
        return Ok(None);
    };
    let budget = guard.resource_budget(connection)?;
    let plan = DeltaBatchPlan::new(snapshot, budget)?;
    advance(connection, snapshot, &completion, guard, budget, plan).map(Some)
}

pub(super) fn start(
    connection: &mut rusqlite::Connection,
    snapshot: &CodeIndexSnapshot,
    session: &durable_clone::CloneSession,
    guard: &PublicationFenceGuard,
) -> Result<DeltaAdvance, StorageError> {
    let budget = guard.resource_budget(connection)?;
    let plan = DeltaBatchPlan::new(snapshot, budget)?;
    let completion =
        durable_clone::start_batched_delta(connection, snapshot, session, plan.len(), guard)?;
    advance(connection, snapshot, &completion, guard, budget, plan)
}

fn advance(
    connection: &mut rusqlite::Connection,
    snapshot: &CodeIndexSnapshot,
    completion: &durable_clone::CloneCompletion,
    guard: &PublicationFenceGuard,
    budget: CodeIndexResourceBudget,
    plan: DeltaBatchPlan<'_>,
) -> Result<DeltaAdvance, StorageError> {
    super::durable_handoff::require_terminal_control_budget(
        snapshot,
        completion,
        plan.len(),
        budget,
    )?;
    let base_batch_count = usize::from(completion.cloned_file_count > 0);
    let checkpoint_batch_count = active_checkpoint_batch_count(connection, snapshot, guard)?;
    let applied = checkpoint_batch_count
        .checked_sub(base_batch_count)
        .filter(|count| *count <= plan.len())
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "durable delta batch progress for scope '{}' is outside its deterministic plan",
                snapshot.source_scope
            ))
        })?;
    let max_steps = completion
        .completed_page_ordinal
        .checked_add(plan.len())
        .ok_or_else(|| capacity(snapshot))?;
    if applied < plan.len() {
        let batch_index = checkpoint_batch_count
            .checked_add(1)
            .ok_or_else(|| capacity(snapshot))?;
        let batch = plan.batch(applied, batch_index)?;
        let checkpoint =
            super::super::batch::apply_batch_with_fence(connection, batch, Some(guard))?;
        let committed = checkpoint
            .batch_count
            .checked_sub(base_batch_count)
            .ok_or_else(|| capacity(snapshot))?;
        if committed != applied + 1 {
            return Err(StorageError::Invariant(format!(
                "durable delta batch {} for scope '{}' did not advance exactly once",
                batch_index, snapshot.source_scope
            )));
        }
        if committed < plan.len() {
            return Ok(DeltaAdvance::Pending {
                completed_steps: completion
                    .completed_page_ordinal
                    .checked_add(committed)
                    .ok_or_else(|| capacity(snapshot))?,
                max_steps,
            });
        }
    }
    durable_clone::finish_batched_delta(connection, snapshot, completion, plan.len(), guard)?;
    Ok(DeltaAdvance::FinalizationRequired)
}

fn active_checkpoint_batch_count(
    connection: &rusqlite::Connection,
    snapshot: &CodeIndexSnapshot,
    guard: &PublicationFenceGuard,
) -> Result<usize, StorageError> {
    let transaction = connection.unchecked_transaction()?;
    guard.validate_target_scope(&transaction, &snapshot.source_scope)?;
    guard.validate(&transaction)?;
    let batch_count = transaction
        .query_row(
            "SELECT batch_count FROM code_repository_index_checkpoints
             WHERE source_scope = ?1 AND repository_id = ?2 AND state = 'indexing'
               AND incremental_summary_json IS NULL",
            [&snapshot.source_scope, &snapshot.repository_id],
            |row| row.get::<_, usize>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "durable delta checkpoint for scope '{}' disappeared",
                snapshot.source_scope
            ))
        })?;
    guard.validate_target_scope(&transaction, &snapshot.source_scope)?;
    guard.validate(&transaction)?;
    transaction.commit()?;
    Ok(batch_count)
}

fn capacity(snapshot: &CodeIndexSnapshot) -> StorageError {
    StorageError::CapacityExceeded(format!(
        "durable delta progress for scope '{}' exceeds platform capacity",
        snapshot.source_scope
    ))
}
