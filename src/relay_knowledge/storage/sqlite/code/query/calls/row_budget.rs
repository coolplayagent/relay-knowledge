//! One execution budget covers selector resolution and both ordered partitions.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::storage::StorageError;
use rusqlite::{Connection, ErrorCode};

const PROGRESS_INTERVAL: i32 = 1_000;
pub(super) const MAX_PROGRESS_CALLBACKS: usize = 4_096;

#[cfg(test)]
thread_local! {
    pub(super) static LAST_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn run<T>(
    connection: &Connection,
    limit: usize,
    query: impl FnOnce() -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    let callbacks = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&callbacks);
    connection.progress_handler(
        PROGRESS_INTERVAL,
        Some(move || observed.fetch_add(1, Ordering::Relaxed) >= limit),
    );
    let result = query();
    connection.progress_handler(0, None::<fn() -> bool>);
    #[cfg(test)]
    LAST_STEPS.set(callbacks.load(Ordering::Relaxed) * PROGRESS_INTERVAL as usize);
    match result {
        Err(StorageError::Sqlite(rusqlite::Error::SqliteFailure(error, _))) if error.code == ErrorCode::OperationInterrupted => Err(StorageError::QueryBudgetExceeded("call query incomplete: SQLite execution budget exhausted; narrow repository path/language filters or select a more specific symbol_snapshot_id".to_owned())),
        result => result,
    }
}

#[cfg(test)]
#[path = "row_budget_tests.rs"]
mod tests;
