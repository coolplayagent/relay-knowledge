//! Bounded query-index construction and repair phase handlers.

use rusqlite::Transaction;

use crate::{
    domain::{
        CodeIndexSession, CodeQueryIndexRepair, CodeQueryIndexRepairResumePhase,
        CodeReferenceSearchQueryIndexRepair, CodeReferenceSearchRebuild,
        code_query_index_repair_state, code_query_index_subphase, code_query_index_subphase_state,
        code_reference_search_query_index_repair_state,
    },
    storage::{
        StorageError,
        sqlite::code::{
            lifecycle::publication_fence::PublicationFenceGuard,
            schema::{
                SearchQueryIndexAdvance, advance_search_query_index_repair,
                advance_search_query_indexes,
            },
        },
    },
};

use super::super::{checkpoint, finalize};
use super::{TransactionAdvance, require_unpublished_finalization_owner};

pub(super) fn repair_query_indexes_during_reference_search(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    reference_search: CodeReferenceSearchRebuild,
) -> Result<Option<TransactionAdvance>, StorageError> {
    let advance = advance_search_query_index_repair(transaction, None, false)?;
    let SearchQueryIndexAdvance::Created { completed_unit, .. } = advance else {
        return Ok(None);
    };
    let next_state = code_reference_search_query_index_repair_state(
        completed_unit,
        reference_search,
    )
    .ok_or_else(|| {
        StorageError::Invariant(format!(
            "query-index repair unit {completed_unit} exceeds the durable reference-search plan"
        ))
    })?;
    checkpoint::compare_and_mark_state(
        transaction,
        &session.source_scope,
        checkpoint_state,
        &next_state,
    )?;
    Ok(Some(TransactionAdvance::Pending(next_state)))
}

pub(super) fn advance_reference_search_query_index_repair(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    repair: CodeReferenceSearchQueryIndexRepair,
    fence: Option<&PublicationFenceGuard>,
) -> Result<TransactionAdvance, StorageError> {
    let fence = fence.ok_or_else(|| {
        StorageError::Invariant(
            "reference-search query-index repair requires a publication fence".to_owned(),
        )
    })?;
    require_unpublished_finalization_owner(transaction, session, fence)?;
    let advance = advance_search_query_index_repair(
        transaction,
        Some(repair.completed_unit),
        repair.requires_legacy_retired_prefix(),
    )?;
    let next_state = match advance {
        SearchQueryIndexAdvance::Created { completed_unit, .. } => {
            repair.next_state(completed_unit).ok_or_else(|| {
                StorageError::Invariant(format!(
                    "query-index repair unit {completed_unit} exceeds the durable reference-search plan"
                ))
            })?
        }
        SearchQueryIndexAdvance::Complete => {
            repair.reference_search.checkpoint_state().ok_or_else(|| {
                StorageError::Invariant(
                    "query-index repair carried a noncanonical reference-search cursor".to_owned(),
                )
            })?
        }
    };
    checkpoint::compare_and_mark_state(
        transaction,
        &session.source_scope,
        checkpoint_state,
        &next_state,
    )?;
    require_unpublished_finalization_owner(transaction, session, fence)?;
    Ok(TransactionAdvance::Pending(next_state))
}

pub(super) fn advance_query_index_phase(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
) -> Result<TransactionAdvance, StorageError> {
    let cursor = code_query_index_subphase(checkpoint_state);
    let completed_unit = cursor.map(|cursor| cursor.completed_unit);
    let require_retired_prefix =
        cursor.is_some_and(|cursor| cursor.requires_legacy_retired_prefix());
    let advance =
        advance_search_query_indexes(transaction, completed_unit, require_retired_prefix)?;
    let next_state = match advance {
        SearchQueryIndexAdvance::Created {
            completed_unit,
            plan_complete,
        } => {
            if plan_complete {
                finalize::phases::BUILD_QUERY_INDEXES.to_owned()
            } else {
                let next_cursor = match cursor {
                    Some(cursor) => cursor.next_state(completed_unit),
                    None => code_query_index_subphase_state(completed_unit),
                };
                next_cursor.ok_or_else(|| {
                    StorageError::Invariant(format!(
                        "query-index finalization unit {completed_unit} exceeds the durable plan"
                    ))
                })?
            }
        }
        SearchQueryIndexAdvance::Complete => finalize::phases::BUILD_QUERY_INDEXES.to_owned(),
    };
    checkpoint::compare_and_mark_state(
        transaction,
        &session.source_scope,
        checkpoint_state,
        &next_state,
    )?;
    Ok(TransactionAdvance::Pending(next_state))
}

/// Repairs any durable coarse checkpoint against the current versioned plan.
/// The token preserves the exact completed phase, so already committed phases
/// never run again after a compatible plan transition.
pub(super) fn repair_query_indexes_after_coarse_checkpoint(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    resume_phase: CodeQueryIndexRepairResumePhase,
) -> Result<Option<TransactionAdvance>, StorageError> {
    let advance = advance_search_query_indexes(transaction, None, false)?;
    let SearchQueryIndexAdvance::Created { completed_unit, .. } = advance else {
        return Ok(None);
    };
    let next_state =
        code_query_index_repair_state(completed_unit, resume_phase).ok_or_else(|| {
            StorageError::Invariant(format!(
                "query-index repair unit {completed_unit} exceeds the durable plan"
            ))
        })?;
    checkpoint::compare_and_mark_state(
        transaction,
        &session.source_scope,
        checkpoint_state,
        &next_state,
    )?;
    Ok(Some(TransactionAdvance::Pending(next_state)))
}

pub(super) fn advance_query_index_repair(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    repair: CodeQueryIndexRepair,
) -> Result<TransactionAdvance, StorageError> {
    let advance = advance_search_query_index_repair(
        transaction,
        Some(repair.completed_unit),
        repair.requires_legacy_retired_prefix(),
    )?;
    let next_state = match advance {
        SearchQueryIndexAdvance::Created { completed_unit, .. } => {
            repair.next_state(completed_unit).ok_or_else(|| {
                StorageError::Invariant(format!(
                    "query-index repair unit {completed_unit} exceeds the durable plan"
                ))
            })?
        }
        SearchQueryIndexAdvance::Complete => repair.resume_phase.checkpoint_state().to_owned(),
    };
    checkpoint::compare_and_mark_state(
        transaction,
        &session.source_scope,
        checkpoint_state,
        &next_state,
    )?;
    Ok(TransactionAdvance::Pending(next_state))
}
