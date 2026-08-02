//! Stable internal facade for checkpointed SQLite code-index batch persistence.

mod checkpoint;
pub(super) mod dependencies;
mod finalize;
mod persistence;
mod session;

pub(super) use persistence::apply_batch;
pub(super) use session::{begin_session, finalize_session};
