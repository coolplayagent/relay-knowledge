//! Stable internal facade for checkpointed SQLite code-index batch persistence.

mod checkpoint;
pub(super) mod dependencies;
pub(in crate::storage::sqlite::code) mod finalize;
mod persistence;
mod session;

pub(super) use persistence::{apply_batch, apply_batch_with_fence};
pub(super) use session::{
    CodeIndexFinalizationAdvance, advance_session, advance_session_with_fence, begin_session,
    begin_session_at_checkpoint, begin_session_at_checkpoint_with_fence, begin_session_with_fence,
    materialize_partitioned_completed_checkpoint,
    reopen_completed_checkpoint_for_partitioned_repair,
};
