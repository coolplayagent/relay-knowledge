//! Wires durable ordinary-reference pages into the fenced session state machine.

use rusqlite::Transaction;

use super::{
    TransactionAdvance, checkpoint, finalization_target_is_unpublished, finalize,
    require_unpublished_finalization_owner, require_unpublished_finalization_target,
};
use crate::{
    domain::{
        CodeIndexSession, CodeReferenceResolution, CodeReferenceResolutionQueryIndexRepair,
        code_reference_resolution_query_index_repair_state, code_reference_resolution_state,
    },
    storage::{
        StorageError,
        sqlite::code::{
            lifecycle::publication_fence::PublicationFenceGuard,
            schema::{SearchQueryIndexAdvance, advance_search_query_index_repair},
        },
    },
};

pub(super) fn initialize(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    expected_reference_count: usize,
    fence: &PublicationFenceGuard,
) -> Result<TransactionAdvance, StorageError> {
    if !finalization_target_is_unpublished(transaction, session, fence)? {
        return Err(StorageError::Invariant(format!(
            "durable reference-resolution pages cannot initialize for queryable scope '{}'",
            session.source_scope
        )));
    }
    require_unpublished_finalization_target(transaction, session, fence)?;
    let advance = finalize::references::initialize_progress(
        transaction,
        &session.source_scope,
        session.resource_budget,
        expected_reference_count,
    )?;
    let result = mark_advance(transaction, session, checkpoint_state, advance)?;
    require_unpublished_finalization_target(transaction, session, fence)?;
    Ok(result)
}

pub(super) fn advance_page(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    resolution: CodeReferenceResolution,
    expected_reference_count: usize,
    fence: Option<&PublicationFenceGuard>,
) -> Result<TransactionAdvance, StorageError> {
    let fence = fence.ok_or_else(|| {
        StorageError::Invariant(
            "durable reference-resolution progress requires a publication fence".to_owned(),
        )
    })?;
    require_unpublished_finalization_owner(transaction, session, fence)?;
    if let Some(repair) = repair_query_indexes(transaction, session, checkpoint_state, resolution)?
    {
        require_unpublished_finalization_owner(transaction, session, fence)?;
        return Ok(repair);
    }
    require_unpublished_finalization_target(transaction, session, fence)?;
    let advance = finalize::references::advance_progress(
        transaction,
        &session.source_scope,
        resolution,
        session.resource_budget,
        expected_reference_count,
    )?;
    let result = mark_advance(transaction, session, checkpoint_state, advance)?;
    require_unpublished_finalization_target(transaction, session, fence)?;
    Ok(result)
}

pub(super) fn advance_query_index_repair(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    repair: CodeReferenceResolutionQueryIndexRepair,
    fence: Option<&PublicationFenceGuard>,
) -> Result<TransactionAdvance, StorageError> {
    let fence = fence.ok_or_else(|| {
        StorageError::Invariant(
            "reference-resolution query-index repair requires a publication fence".to_owned(),
        )
    })?;
    require_unpublished_finalization_owner(transaction, session, fence)?;
    let advance = advance_search_query_index_repair(
        transaction,
        Some(repair.completed_unit),
        repair.requires_legacy_retired_prefix(),
    )?;
    let next_state = match advance {
        SearchQueryIndexAdvance::Created {
            completed_unit, ..
        } => repair.next_state(completed_unit).ok_or_else(|| {
            StorageError::Invariant(format!(
                "query-index repair unit {completed_unit} exceeds the durable reference-resolution plan"
            ))
        })?,
        SearchQueryIndexAdvance::Complete => repair
            .reference_resolution
            .checkpoint_state()
            .ok_or_else(|| {
                StorageError::Invariant(
                    "query-index repair carried a noncanonical reference-resolution cursor"
                        .to_owned(),
                )
            })?,
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

fn repair_query_indexes(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    resolution: CodeReferenceResolution,
) -> Result<Option<TransactionAdvance>, StorageError> {
    let advance = advance_search_query_index_repair(transaction, None, false)?;
    let SearchQueryIndexAdvance::Created { completed_unit, .. } = advance else {
        return Ok(None);
    };
    let next_state = code_reference_resolution_query_index_repair_state(completed_unit, resolution)
        .ok_or_else(|| {
            StorageError::Invariant(format!(
                "query-index repair unit {completed_unit} exceeds the durable reference-resolution plan"
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

fn mark_advance(
    transaction: &Transaction<'_>,
    session: &CodeIndexSession,
    checkpoint_state: &str,
    advance: finalize::references::ReferenceResolutionAdvance,
) -> Result<TransactionAdvance, StorageError> {
    let next_state = match advance {
        finalize::references::ReferenceResolutionAdvance::Pending {
            completed_page_ordinal,
            completed_reference_count,
            cursor_reference_id,
        } => code_reference_resolution_state(
            completed_page_ordinal,
            completed_reference_count,
            cursor_reference_id.as_deref(),
        )
        .ok_or_else(|| {
            StorageError::Invariant(
                "reference-resolution page produced a noncanonical checkpoint token".to_owned(),
            )
        })?,
        finalize::references::ReferenceResolutionAdvance::Complete => {
            finalize::phases::RESOLVE_REFERENCES.to_owned()
        }
    };
    checkpoint::compare_and_mark_state(
        transaction,
        &session.source_scope,
        checkpoint_state,
        &next_state,
    )?;
    Ok(TransactionAdvance::Pending(next_state))
}
