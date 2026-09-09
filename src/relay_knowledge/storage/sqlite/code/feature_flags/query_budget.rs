//! A shared SQLite work budget spans seed refills, binding closure and hydration.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::storage::StorageError;
use rusqlite::{Connection, ErrorCode};

const PROGRESS_INTERVAL: i32 = 1000;
const MAX_CALLBACKS: usize = 4096;

#[cfg(test)]
thread_local! {
    static LAST_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn run<T>(
    connection: &Connection,
    query: impl FnOnce() -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    let callbacks = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&callbacks);
    connection.progress_handler(
        PROGRESS_INTERVAL,
        Some(move || observed.fetch_add(1, Ordering::Relaxed) >= MAX_CALLBACKS),
    );
    let result = query();
    connection.progress_handler(0, None::<fn() -> bool>);
    let count = callbacks.load(Ordering::Relaxed);
    #[cfg(test)]
    LAST_STEPS.set(count * PROGRESS_INTERVAL as usize);
    match result {
        Err(StorageError::Sqlite(rusqlite::Error::SqliteFailure(error, _)))
            if error.code == ErrorCode::OperationInterrupted && count > MAX_CALLBACKS =>
        {
            Err(StorageError::QueryBudgetExceeded(
                "configuration query incomplete: SQLite execution budget exhausted; narrow the query terms or repository path/language filters".to_owned(),
            ))
        }
        result => result,
    }
}

#[cfg(test)]
#[path = "query_budget_tests.rs"]
mod tests;
