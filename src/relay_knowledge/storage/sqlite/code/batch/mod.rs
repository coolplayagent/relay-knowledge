//! Stable internal facade for checkpointed SQLite code-index batch persistence.

mod checkpoint;
pub(super) mod dependencies;
mod finalize;
mod persistence;
mod session;

pub(super) use persistence::{apply_batch, apply_batch_with_fence};
pub(super) use session::{
    begin_session, begin_session_with_fence, finalize_session, finalize_session_with_fence,
};
