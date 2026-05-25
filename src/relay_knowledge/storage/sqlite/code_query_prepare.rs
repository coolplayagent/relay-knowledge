use std::{thread, time::Duration};

use rusqlite::{Connection, Statement};

use crate::{
    domain::{CodeQueryKind, CodeRetrievalRequest},
    storage::StorageError,
};

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
    request: &CodeRetrievalRequest,
    error: &StorageError,
) -> bool {
    code_search_plannable_outage_reason(request, error).is_some()
}

pub(super) fn code_search_plannable_outage_reason(
    request: &CodeRetrievalRequest,
    error: &StorageError,
) -> Option<String> {
    if !code_query_can_plan_source_fallback(request) {
        return None;
    }
    match error {
        StorageError::Sqlite(error) => {
            code_search_read_model_unavailable_message(&error.to_string())
                .then(|| format!("code search read model unavailable: {error}"))
        }
        _ => None,
    }
}

fn code_query_can_plan_source_fallback(request: &CodeRetrievalRequest) -> bool {
    match request.code_query_kind {
        CodeQueryKind::Definition => code_query_definition_identity(&request.query).is_some(),
        CodeQueryKind::References | CodeQueryKind::Hybrid => {
            code_query_source_identifier(&request.query).is_some()
        }
        CodeQueryKind::Symbol
        | CodeQueryKind::Imports
        | CodeQueryKind::Callers
        | CodeQueryKind::Callees
        | CodeQueryKind::Sbom
        | CodeQueryKind::Impact => false,
    }
}

fn code_query_definition_identity(query: &str) -> Option<&str> {
    let mut identity = None;
    for raw_token in query.split_whitespace().map(str::trim) {
        if raw_token.contains('/') || raw_token.contains('\\') {
            continue;
        }
        let terms = raw_token
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        if let Some(term) = terms
            .last()
            .filter(|term| code_query_single_identifier(term))
        {
            identity = Some(*term);
        }
    }

    identity
}

fn code_query_source_identifier(query: &str) -> Option<&str> {
    let identity = code_query_definition_identity(query)?;
    (query.split_whitespace().count() == 1).then_some(identity)
}

fn code_query_single_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }

    characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
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
}
