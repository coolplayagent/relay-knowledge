//! Bounded finalization progress shared by storage implementations.
use crate::{domain::CodeIndexSummary, storage::StorageError};

/// Stable coarse states in the durable code-index finalization plan.
pub const CODE_INDEX_FINALIZATION_COARSE_PHASE_COUNT: usize = 12;

/// Hard bound for missing index units, coarse phases, and terminal observation.
pub const CODE_INDEX_FINALIZATION_MAX_STEPS: usize = crate::domain::CODE_QUERY_INDEX_PLAN_UNIT_COUNT
    + CODE_INDEX_FINALIZATION_COARSE_PHASE_COUNT
    + 2;

/// Derives the hard finalization quantum bound including worst-case
/// byte-limited reference resolution plus reference-search cleanup, group
/// discovery, and build pages.
pub fn code_index_finalization_max_steps(
    committed_reference_count: usize,
    committed_symbol_count: usize,
) -> Result<usize, StorageError> {
    committed_reference_count
        .checked_mul(4)
        .and_then(|pages| pages.checked_add(committed_symbol_count))
        .and_then(|pages| pages.checked_add(CODE_INDEX_FINALIZATION_MAX_STEPS + 6))
        .ok_or_else(|| {
            StorageError::CapacityExceeded(
                "reference-resolution and search finalization step bound exceeds platform capacity"
                    .to_owned(),
            )
        })
}

/// Result of advancing one durable code-index finalization writer quantum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeIndexFinalizationStep {
    Pending { checkpoint_state: String },
    Ready(Box<CodeIndexSummary>),
}
