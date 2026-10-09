//! Durable software-feedback journal contract, independent of graph storage.

use std::{error::Error, fmt, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};

use crate::domain::feedback::{FeedbackPolicy, FeedbackRecord};

/// Maximum retained reports; reaching capacity preserves existing evidence.
pub const MAX_FEEDBACK_RECORDS: usize = 1_000;

/// Maximum encoded journal size, including original evidence and publication policy.
pub const MAX_FEEDBACK_JOURNAL_BYTES: usize = 16 * 1024 * 1024;

/// Single atomic snapshot of feedback, publication authorization, and quota accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackJournal {
    pub schema_version: u32,
    pub policy: FeedbackPolicy,
    pub records: Vec<FeedbackRecord>,
    pub quota_window_start_ms: u64,
    pub quota_attempts: u32,
}

impl Default for FeedbackJournal {
    fn default() -> Self {
        Self {
            schema_version: 1,
            policy: FeedbackPolicy::default(),
            records: Vec::new(),
            quota_window_start_ms: 0,
            quota_attempts: 0,
        }
    }
}

/// Stable storage failure categories for actionable CLI/API diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackStoreErrorKind {
    Busy,
    Capacity,
    InvalidData,
    Io,
}

/// Storage diagnostics never include report content or authentication material.
#[derive(Debug, Clone)]
pub struct FeedbackStoreError {
    pub kind: FeedbackStoreErrorKind,
    pub message: String,
}

impl fmt::Display for FeedbackStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for FeedbackStoreError {}

/// Async persistence operation run outside the async executor's blocking hot path.
pub type FeedbackStoreFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, FeedbackStoreError>> + Send + 'a>>;

/// Exclusive publication transaction retained through reconciliation and sending.
///
/// `commit` durably publishes its snapshot while retaining exclusivity. A caller
/// must persist an ambiguous-send state before issuing a remote mutation. Drop
/// releases the lock, including cancellation and panic paths; uncommitted local
/// changes are discarded. Implementations reject conflicting writers promptly.
pub trait FeedbackTransaction: Send {
    fn snapshot(&self) -> &FeedbackJournal;
    fn snapshot_mut(&mut self) -> &mut FeedbackJournal;
    fn commit(&mut self) -> FeedbackStoreFuture<'_, ()>;
}

/// Port injected by bootstrap; feedback never becomes an accepted graph fact.
pub trait FeedbackStore: Send + Sync {
    fn begin(&self) -> FeedbackStoreFuture<'_, Box<dyn FeedbackTransaction>>;
}
