//! Validates durable clone identity, counters, phase position, and checkpoint parity.

use rusqlite::{OptionalExtension, Transaction};

use crate::{
    domain::{CodeIncrementalClonePhase, CodeIndexResourceBudget, code_incremental_clone},
    storage::StorageError,
};

use super::{
    CloneIdentity, base, durable_page_byte_limit, progress, table_count, validate_base_scope,
    validate_staged_target,
};
use crate::storage::sqlite::code::lifecycle::publication_fence::PublicationFenceGuard;

pub(super) fn validate_progress(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
    base_scope: &str,
    guard: &PublicationFenceGuard,
    budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    let phase = validate_progress_state(transaction, current, identity, base_scope, guard, budget)?;
    let expected_checkpoint = progress::checkpoint_state(current)?;
    let checkpoint = transaction
        .query_row(
            "SELECT state FROM code_repository_index_checkpoints WHERE source_scope = ?1",
            [&identity.source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let parsed_checkpoint = checkpoint
        .as_deref()
        .and_then(code_incremental_clone)
        .filter(|parsed| {
            parsed.phase == phase
                && parsed.table_ordinal == current.table_ordinal
                && parsed.completed_page_ordinal == current.completed_page_ordinal
                && parsed.scanned_total_rows == current.scanned_total_rows
        });
    if checkpoint.as_deref() != Some(expected_checkpoint.as_str()) || parsed_checkpoint.is_none() {
        return Err(StorageError::Invariant(format!(
            "incremental clone progress and checkpoint for scope '{}' diverged",
            identity.source_scope
        )));
    }
    Ok(())
}

pub(super) fn validate_delta_progress(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
    guard: &PublicationFenceGuard,
    budget: CodeIndexResourceBudget,
) -> Result<(), StorageError> {
    let base_scope = current.base_scope.as_str();
    let phase = validate_progress_state(transaction, current, identity, base_scope, guard, budget)?;
    let checkpoint = transaction
        .query_row(
            "SELECT state, incremental_summary_json
             FROM code_repository_index_checkpoints WHERE source_scope = ?1",
            [&identity.source_scope],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
    if phase != CodeIncrementalClonePhase::CloneComplete
        || checkpoint != Some(("indexing".to_owned(), None))
    {
        return Err(StorageError::Invariant(format!(
            "incremental clone delta checkpoint for scope '{}' is not resumable",
            identity.source_scope
        )));
    }
    Ok(())
}

fn validate_progress_state(
    transaction: &Transaction<'_>,
    current: &progress::CloneProgress,
    identity: &CloneIdentity,
    base_scope: &str,
    guard: &PublicationFenceGuard,
    budget: CodeIndexResourceBudget,
) -> Result<CodeIncrementalClonePhase, StorageError> {
    let identity_matches = current.source_scope == identity.source_scope
        && current.repository_id == identity.repository_id
        && current.base_scope == base_scope
        && current.task_id == guard.task_id()
        && current.delta_digest == identity.delta_digest
        && current.page_row_limit == budget.max_rows_per_batch
        && current.page_byte_limit == durable_page_byte_limit(budget);
    if !identity_matches {
        return Err(StorageError::Invariant(format!(
            "incremental clone progress identity for scope '{}' does not match the live task",
            identity.source_scope
        )));
    }
    let phase = current.typed_phase()?;
    let phase_position_is_valid = match phase {
        CodeIncrementalClonePhase::Tables => current.table_ordinal < table_count(),
        CodeIncrementalClonePhase::Search | CodeIncrementalClonePhase::CloneComplete => {
            current.table_ordinal == table_count()
        }
    };
    let completed_table_proof_is_valid = match (
        phase,
        current.completed_table_ordinal,
        current.expected_table_rows,
    ) {
        (CodeIncrementalClonePhase::Tables, None, None) => current.table_ordinal == 0,
        (CodeIncrementalClonePhase::Tables, Some(completed), Some(expected)) => {
            completed.checked_add(1) == Some(current.table_ordinal)
                && expected <= current.scanned_total_rows
        }
        (CodeIncrementalClonePhase::Search, Some(completed), Some(expected)) => {
            completed.checked_add(1) == Some(table_count())
                && expected <= current.scanned_total_rows
        }
        (CodeIncrementalClonePhase::CloneComplete, Some(completed), Some(expected)) => {
            completed == table_count() && expected <= current.scanned_total_rows
        }
        _ => false,
    };
    let cloned_counter_total = current
        .cloned_file_count
        .saturating_add(current.cloned_symbol_count)
        .saturating_add(current.cloned_reference_count)
        .saturating_add(current.cloned_chunk_count)
        .saturating_add(current.cloned_diagnostic_count)
        .saturating_add(current.cloned_reference_group_count)
        .saturating_add(current.cloned_search_document_count.saturating_mul(2));
    let base_manifest = base::manifest_header(transaction, base_scope)?;
    let base_step_proof = base::step_proof(transaction, base_scope)?;
    if current.copied_table_rows > current.scanned_table_rows
        || current.copied_total_rows > current.scanned_total_rows.saturating_mul(2)
        || cloned_counter_total > current.copied_total_rows
        || current.base_manifest_reference_count != base_manifest.0
        || current.base_manifest_group_count != base_manifest.1
        || current.base_source_fact_row_upper_bound != base_step_proof.source_fact_row_upper_bound
        || current.scanned_reference_occurrence_count > current.base_manifest_reference_count
        || current.scanned_reference_row_count > current.base_manifest_reference_count
        || current.scanned_reference_group_count > current.base_manifest_group_count
        || current.scanned_reference_search_owner_count > current.base_manifest_group_count
        || !phase_position_is_valid
        || !completed_table_proof_is_valid
    {
        return Err(StorageError::Invariant(format!(
            "incremental clone progress counters for scope '{}' are invalid",
            identity.source_scope
        )));
    }
    if phase == CodeIncrementalClonePhase::CloneComplete
        && (current.scanned_reference_occurrence_count != current.base_manifest_reference_count
            || current.scanned_reference_row_count != current.base_manifest_reference_count
            || current.scanned_reference_group_count != current.base_manifest_group_count
            || current.scanned_reference_search_owner_count != current.base_manifest_group_count)
    {
        return Err(StorageError::Invariant(format!(
            "incremental clone projection proof for scope '{}' is incomplete",
            identity.source_scope
        )));
    }
    validate_base_scope(transaction, identity, base_scope)?;
    validate_staged_target(transaction, identity)?;
    Ok(phase)
}
