use std::{thread, time::Duration};

use rusqlite::ErrorCode;

use crate::storage::StorageError;

const SQLITE_TRANSIENT_RETRY_DELAYS_MS: [u64; 6] = [10, 30, 90, 270, 810, 1620];

pub(super) fn retry_sqlite_transient<T>(
    mut operation: impl FnMut() -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    for delay_ms in SQLITE_TRANSIENT_RETRY_DELAYS_MS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if sqlite_transient_error_is_retryable(&error) => {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(error),
        }
    }

    operation()
}

fn sqlite_transient_error_is_retryable(error: &StorageError) -> bool {
    match error {
        StorageError::Sqlite(error) => sqlite_error_is_retryable(error),
        _ => false,
    }
}

fn sqlite_error_is_retryable(error: &rusqlite::Error) -> bool {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => {
            matches!(
                failure.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            ) || sqlite_transient_message_is_retryable(error.to_string().as_str())
        }
        _ => sqlite_transient_message_is_retryable(error.to_string().as_str()),
    }
}

fn sqlite_transient_message_is_retryable(message: &str) -> bool {
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database schema is locked")
        || message.contains("vtable constructor failed")
}

#[cfg(test)]
mod tests {
    use super::sqlite_transient_message_is_retryable;

    #[test]
    fn sqlite_retry_messages_are_limited_to_transient_lock_failures() {
        assert!(sqlite_transient_message_is_retryable(
            "sqlite operation failed: database is locked"
        ));
        assert!(sqlite_transient_message_is_retryable(
            "database schema is locked"
        ));
        assert!(sqlite_transient_message_is_retryable(
            "vtable constructor failed: code_repository_search"
        ));
        assert!(sqlite_transient_message_is_retryable(
            "vtable constructor failed: file_index_search"
        ));
        assert!(!sqlite_transient_message_is_retryable(
            "no such table: code_repository_search"
        ));
    }
}
