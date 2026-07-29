use super::{
    code_query_definition_identity, code_search_error_can_use_empty_results,
    code_search_prepare_error_message_is_retryable, code_search_storage_error_is_retryable,
};
use crate::{
    domain::{CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy},
    storage::StorageError,
};

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
        &request("find rk_handler", CodeQueryKind::Definition),
        &StorageError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("no such table: code_repository_search".to_owned()),
        ))
    ));
    assert!(code_search_error_can_use_empty_results(
        &request("rk_handler", CodeQueryKind::Hybrid),
        &StorageError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("no such module: fts5".to_owned()),
        ))
    ));
    assert!(code_search_error_can_use_empty_results(
        &request("rk_handler", CodeQueryKind::References),
        &StorageError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("no such module: fts5".to_owned()),
        ))
    ));
    assert!(code_search_error_can_use_empty_results(
        &request("rk_handler", CodeQueryKind::References),
        &StorageError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".to_owned()),
        ))
    ));
    assert!(!code_search_error_can_use_empty_results(
        &request("find rk_handler", CodeQueryKind::Definition),
        &StorageError::InvalidInput("no such table: code_repository_search".to_owned())
    ));
}

#[test]
fn unavailable_code_search_read_model_propagates_without_source_fallback() {
    let error = StorageError::Sqlite(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
        Some("no such module: fts5".to_owned()),
    ));

    assert!(!code_search_error_can_use_empty_results(
        &request("rk_handler", CodeQueryKind::Symbol),
        &error
    ));
    assert!(!code_search_error_can_use_empty_results(
        &request("rk_handler", CodeQueryKind::Callers),
        &error
    ));
    assert!(!code_search_error_can_use_empty_results(
        &request("rk_handler", CodeQueryKind::Callees),
        &error
    ));
    assert!(!code_search_error_can_use_empty_results(
        &request("rk_handler", CodeQueryKind::Imports),
        &error
    ));
    assert!(!code_search_error_can_use_empty_results(
        &request("src/rk_handler.rs", CodeQueryKind::Definition),
        &error
    ));
    assert!(!code_search_error_can_use_empty_results(
        &request("find rk_handler", CodeQueryKind::Hybrid),
        &error
    ));
}

#[test]
fn definition_fallback_identity_uses_query_target() {
    assert_eq!(
        code_query_definition_identity("find rk_handler"),
        Some("rk_handler")
    );
    assert_eq!(
        code_query_definition_identity("show service::rk_handler"),
        Some("rk_handler")
    );
    assert_eq!(code_query_definition_identity("src/rk_handler.rs"), None);
}

fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new()).unwrap(),
        kind,
        10,
        FreshnessPolicy::AllowStale,
    )
    .unwrap()
}
