//! Bounded admission and cooperative cancellation for read-only parser previews.

use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::application::code_repository::blocking::run_blocking_code;
use crate::{
    api::{ApiError, ErrorKind},
    code::preview_repository_scope_cancellable,
    domain::{CodeRepositoryRegistration, CodeRepositoryScopePreview, CodeRepositorySelector},
};

static PREVIEW_PERMITS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));
const PREVIEW_QUEUE_TIMEOUT: Duration = Duration::from_secs(5);
const PREVIEW_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

struct CancelPreview(Arc<AtomicBool>);

impl Drop for CancelPreview {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(super) async fn validate_scope(
    registration: CodeRepositoryRegistration,
    selector: CodeRepositorySelector,
) -> Result<CodeRepositoryScopePreview, ApiError> {
    run_preview_worker(
        move |cancelled| {
            preview_repository_scope_cancellable(&registration, &selector, || {
                cancelled.load(Ordering::Relaxed)
            })
        },
        Arc::clone(&PREVIEW_PERMITS),
        PREVIEW_QUEUE_TIMEOUT,
        PREVIEW_RESPONSE_TIMEOUT,
    )
    .await
}

async fn run_preview_worker<T: Send + 'static>(
    operation: impl FnOnce(Arc<AtomicBool>) -> Result<T, crate::code::CodeIndexError> + Send + 'static,
    permits: Arc<Semaphore>,
    queue_timeout: Duration,
    response_timeout: Duration,
) -> Result<T, ApiError> {
    let permit = tokio::time::timeout(queue_timeout, permits.acquire_owned())
        .await
        .map_err(|_| {
            ApiError::qos_rejected("repository preview queue exceeded its admission deadline")
        })?
        .map_err(|_| ApiError::storage_unavailable("repository preview queue is unavailable"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancel_on_drop = CancelPreview(Arc::clone(&cancelled));
    let worker = run_blocking_code(move || {
        // A dropped request stops at the next batch boundary, retaining admission until then.
        let _permit = permit;
        operation(cancelled)
    });
    tokio::time::timeout(response_timeout, worker).await.map_err(|_| ApiError {
        error_kind: ErrorKind::Timeout,
        message: "repository preview exceeded its parsing deadline; narrow the registered scope and retry".to_owned(),
        metadata: None,
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn saturated_preview_queue_rejects_without_starting_parser() {
        let result = run_preview_worker(
            |_| -> Result<(), crate::code::CodeIndexError> { panic!("parser must not start") },
            Arc::new(Semaphore::new(0)),
            Duration::ZERO,
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(result.unwrap_err().error_kind, ErrorKind::QosRejected);
    }

    #[tokio::test]
    async fn timed_out_preview_keeps_permit_until_worker_observes_cancellation() {
        let permits = Arc::new(Semaphore::new(1));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let worker_permits = Arc::clone(&permits);
        let task = tokio::spawn(run_preview_worker(
            move |cancelled| {
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(cancelled.load(Ordering::Relaxed));
                finished_tx.send(()).unwrap();
                Ok(())
            },
            worker_permits,
            Duration::from_secs(1),
            Duration::from_millis(100),
        ));
        started_rx.await.unwrap();
        assert_eq!(
            task.await.unwrap().unwrap_err().error_kind,
            ErrorKind::Timeout
        );
        assert_eq!(permits.available_permits(), 0);
        release_tx.send(()).unwrap();
        finished_rx.await.unwrap();
        let _released = tokio::time::timeout(Duration::from_secs(1), permits.acquire())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn closed_preview_queue_returns_an_observable_error() {
        let permits = Arc::new(Semaphore::new(1));
        permits.close();
        let error = run_preview_worker(
            |_| Ok(()),
            permits,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("queue is unavailable"));
    }

    #[test]
    fn dropped_preview_signals_cancellation_to_the_worker() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let guard = CancelPreview(Arc::clone(&cancelled));
        assert!(!cancelled.load(Ordering::Relaxed));
        drop(guard);
        assert!(cancelled.load(Ordering::Relaxed));
    }
}
