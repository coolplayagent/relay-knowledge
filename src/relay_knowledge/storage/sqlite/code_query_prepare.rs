use std::{thread, time::Duration};

use rusqlite::{Connection, Statement};

use crate::storage::StorageError;

const CODE_SEARCH_PREPARE_RETRY_DELAYS_MS: [u64; 3] = [4, 12, 36];
const CODE_SEARCH_OPERATION_RETRY_DELAYS_MS: [u64; 4] = [10, 30, 90, 270];

pub(super) fn retry_code_search_operation<T>(
    mut operation: impl FnMut() -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    for attempt in 0..=CODE_SEARCH_OPERATION_RETRY_DELAYS_MS.len() {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error)
                if attempt < CODE_SEARCH_OPERATION_RETRY_DELAYS_MS.len()
                    && code_search_storage_error_is_retryable(&error) =>
            {
                thread::sleep(Duration::from_millis(
                    CODE_SEARCH_OPERATION_RETRY_DELAYS_MS[attempt],
                ));
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("bounded code search operation retry loop always returns")
}

pub(super) fn prepare_code_search_statement<'connection>(
    connection: &'connection Connection,
    sql: &str,
) -> Result<Statement<'connection>, StorageError> {
    for attempt in 0..=CODE_SEARCH_PREPARE_RETRY_DELAYS_MS.len() {
        match connection.prepare(sql) {
            Ok(statement) => return Ok(statement),
            Err(error)
                if attempt < CODE_SEARCH_PREPARE_RETRY_DELAYS_MS.len()
                    && code_search_prepare_error_is_retryable(&error) =>
            {
                thread::sleep(Duration::from_millis(
                    CODE_SEARCH_PREPARE_RETRY_DELAYS_MS[attempt],
                ));
            }
            Err(error) => return Err(StorageError::from(error)),
        }
    }

    unreachable!("bounded code search prepare retry loop always returns")
}

fn code_search_prepare_error_is_retryable(error: &rusqlite::Error) -> bool {
    code_search_prepare_error_message_is_retryable(&error.to_string())
}

fn code_search_storage_error_is_retryable(error: &StorageError) -> bool {
    match error {
        StorageError::Sqlite(error) => code_search_prepare_error_is_retryable(error),
        _ => false,
    }
}

fn code_search_prepare_error_message_is_retryable(message: &str) -> bool {
    message.contains("vtable constructor failed: code_repository_search")
        || message.contains("database schema is locked")
        || message.contains("database is locked")
}

#[cfg(test)]
mod tests {
    use super::{
        code_search_prepare_error_message_is_retryable, code_search_storage_error_is_retryable,
    };
    use crate::storage::StorageError;

    #[test]
    fn code_search_prepare_retry_is_limited_to_transient_search_open_errors() {
        assert!(code_search_prepare_error_message_is_retryable(
            "vtable constructor failed: code_repository_search"
        ));
        assert!(code_search_prepare_error_message_is_retryable(
            "database schema is locked"
        ));
        assert!(!code_search_prepare_error_message_is_retryable(
            "no such table: code_repository_search"
        ));
    }

    #[test]
    fn code_search_operation_retry_only_wraps_sqlite_transients() {
        assert!(!code_search_storage_error_is_retryable(
            &StorageError::InvalidInput("database is locked".to_owned())
        ));
    }
}
