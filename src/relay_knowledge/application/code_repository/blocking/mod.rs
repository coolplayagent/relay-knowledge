//! Isolates blocking code-index operations from async runtime executors and maps their errors.

use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use tokio::sync::Semaphore;

use crate::{
    api::{ApiError, ErrorKind},
    code::CodeIndexError,
    domain::DomainError,
};

const DOMAIN_PROJECTION_QUEUE_TIMEOUT: Duration = Duration::from_secs(5);
const DOMAIN_PROJECTION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONCURRENT_DOMAIN_PROJECTIONS: usize = 4;

static DOMAIN_PROJECTION_PERMITS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_CONCURRENT_DOMAIN_PROJECTIONS)));

pub(super) async fn run_blocking_code<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CodeIndexError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
        .map_err(code_api_error)
}

pub(super) async fn run_blocking_domain<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, DomainError> + Send + 'static,
{
    run_blocking_domain_with_policy(
        operation,
        Arc::clone(&DOMAIN_PROJECTION_PERMITS),
        DOMAIN_PROJECTION_QUEUE_TIMEOUT,
        DOMAIN_PROJECTION_RESPONSE_TIMEOUT,
    )
    .await
}

async fn run_blocking_domain_with_policy<T, F>(
    operation: F,
    permits: Arc<Semaphore>,
    queue_timeout: Duration,
    response_timeout: Duration,
) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, DomainError> + Send + 'static,
{
    let permit = tokio::time::timeout(queue_timeout, permits.acquire_owned())
        .await
        .map_err(|_| {
            ApiError::qos_rejected("domain projection queue remained saturated past its deadline")
        })?
        .map_err(|_| ApiError::storage_unavailable("domain projection queue is unavailable"))?;
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        operation()
    });
    // Tokio cannot forcibly cancel a running blocking closure. On a response timeout the
    // detached worker finishes in the background and keeps its permit until it returns, so
    // timed-out work cannot silently expand the blocking concurrency budget.
    tokio::time::timeout(response_timeout, task)
        .await
        .map_err(|_| ApiError {
            error_kind: ErrorKind::Timeout,
            message: "domain projection exceeded its response deadline; bounded blocking work may still finish in the background".to_owned(),
            metadata: None,
        })?
        .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
        .map_err(|error| ApiError::invalid_argument(error.to_string()))
}

fn code_api_error(error: CodeIndexError) -> ApiError {
    match error {
        CodeIndexError::InvalidInput(message) => ApiError::invalid_argument(message),
        CodeIndexError::Git { .. } | CodeIndexError::Io(_) | CodeIndexError::TreeSitter(_) => {
            ApiError::storage_unavailable(error.to_string())
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
