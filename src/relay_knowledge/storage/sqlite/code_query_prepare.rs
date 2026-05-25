use std::{thread, time::Duration};

use rusqlite::{Connection, Statement};

use crate::{domain::CodeQueryKind, storage::StorageError};

const CODE_SEARCH_PREPARE_RETRY_DELAYS_MS: [u64; 3] = [4, 12, 36];
const CODE_SEARCH_OPERATION_RETRY_DELAYS_MS: [u64; 4] = [10, 30, 90, 270];

pub(super) fn retry_code_search_operation<T>(
    mut operation: impl FnMut() -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    for delay_ms in CODE_SEARCH_OPERATION_RETRY_DELAYS_MS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if code_search_storage_error_is_retryable(&error) => {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(error),
        }
    }

    operation()
}

pub(super) fn code_search_error_can_use_empty_results(
    kind: CodeQueryKind,
    error: &StorageError,
) -> bool {
    if !code_query_kind_has_source_fallback(kind) {
        return false;
    }
    match error {
        StorageError::Sqlite(error) => {
            code_search_read_model_unavailable_message(&error.to_string())
        }
        _ => false,
    }
}

fn code_query_kind_has_source_fallback(kind: CodeQueryKind) -> bool {
    matches!(
        kind,
        CodeQueryKind::Definition
            | CodeQueryKind::References
            | CodeQueryKind::Imports
            | CodeQueryKind::Hybrid
    )
}

pub(super) fn prepare_code_search_statement<'connection>(
    connection: &'connection Connection,
    sql: &str,
) -> Result<Statement<'connection>, StorageError> {
    for delay_ms in CODE_SEARCH_PREPARE_RETRY_DELAYS_MS {
        match connection.prepare(sql) {
            Ok(statement) => return Ok(statement),
            Err(error) if code_search_prepare_error_is_retryable(&error) => {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(StorageError::from(error)),
        }
    }

    connection.prepare(sql).map_err(StorageError::from)
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

fn code_search_read_model_unavailable_message(message: &str) -> bool {
    message.contains("vtable constructor failed: code_repository_search")
        || message.contains("no such table: code_repository_search")
        || message.contains("no such module: fts5")
}

#[cfg(test)]
mod tests {
    use super::{
        code_query_kind_has_source_fallback, code_search_error_can_use_empty_results,
        code_search_prepare_error_message_is_retryable, code_search_storage_error_is_retryable,
    };
    use crate::{domain::CodeQueryKind, storage::StorageError};

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

    #[test]
    fn unavailable_code_search_read_model_can_fall_back_to_empty_results() {
        assert!(code_search_error_can_use_empty_results(
            CodeQueryKind::Definition,
            &StorageError::Sqlite(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                Some("no such table: code_repository_search".to_owned()),
            ))
        ));
        assert!(code_search_error_can_use_empty_results(
            CodeQueryKind::Hybrid,
            &StorageError::Sqlite(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
                Some("no such module: fts5".to_owned()),
            ))
        ));
        assert!(!code_search_error_can_use_empty_results(
            CodeQueryKind::Definition,
            &StorageError::InvalidInput("no such table: code_repository_search".to_owned())
        ));
    }

    #[test]
    fn unavailable_code_search_read_model_propagates_without_source_fallback() {
        let error = StorageError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("no such module: fts5".to_owned()),
        ));

        assert!(code_query_kind_has_source_fallback(
            CodeQueryKind::References
        ));
        assert!(!code_query_kind_has_source_fallback(CodeQueryKind::Symbol));
        assert!(!code_search_error_can_use_empty_results(
            CodeQueryKind::Symbol,
            &error
        ));
        assert!(!code_search_error_can_use_empty_results(
            CodeQueryKind::Callers,
            &error
        ));
        assert!(!code_search_error_can_use_empty_results(
            CodeQueryKind::Callees,
            &error
        ));
    }
}
