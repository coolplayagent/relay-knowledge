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
    code_search_read_model_unavailable_reason(error)
}

pub(super) fn code_search_read_model_unavailable_reason(error: &StorageError) -> Option<String> {
    match error {
        StorageError::Sqlite(error) => {
            let message = error.to_string();
            if code_search_read_model_unavailable_message(&message) {
                Some(format!("code search read model unavailable: {error}"))
            } else if code_search_prepare_error_message_is_retryable(&message) {
                Some(format!(
                    "code search read model temporarily unavailable: {error}"
                ))
            } else {
                None
            }
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
#[path = "prepare_tests.rs"]
mod tests;
