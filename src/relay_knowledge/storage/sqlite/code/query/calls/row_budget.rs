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
        Err(StorageError::Sqlite(rusqlite::Error::SqliteFailure(error, _))) if error.code == ErrorCode::OperationInterrupted => Err(StorageError::InvalidInput("call query incomplete: SQLite execution budget exhausted; narrow repository path/language filters or select a more specific symbol_snapshot_id".to_owned())),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_is_explicit_and_handler_is_cleared_on_every_result() {
        let connection = Connection::open_in_memory().unwrap();
        let sql = "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10000) SELECT sum(x) FROM n";
        let error = run(&connection, 0, || {
            connection
                .query_row(sql, [], |r| r.get::<_, i64>(0))
                .map_err(StorageError::from)
        })
        .unwrap_err();
        assert!(error.to_string().contains("call query incomplete"));
        assert_eq!(
            connection
                .query_row(sql, [], |r| r.get::<_, i64>(0))
                .unwrap(),
            50_005_000
        );
        assert!(
            run::<()>(&connection, 0, || Err(StorageError::InvalidInput(
                "test".into()
            )))
            .is_err()
        );
        assert_eq!(
            connection
                .query_row(sql, [], |r| r.get::<_, i64>(0))
                .unwrap(),
            50_005_000
        );
        assert_eq!(
            run(&connection, MAX_PROGRESS_CALLBACKS, || Ok(3)).unwrap(),
            3
        );
        assert_eq!(
            connection
                .query_row(sql, [], |r| r.get::<_, i64>(0))
                .unwrap(),
            50_005_000
        );
    }
}
