//! Bounded, private, atomic file journal for software feedback.
//!
//! A stable OS file lock serializes writers across processes through the full
//! publication workflow. Disk operations and JSON work run on explicit blocking
//! workers, and four process-wide permits bound transactions and worker admission.

use std::sync::{Arc, OnceLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{
    paths::{RuntimePaths, StorageDirectoryAccess},
    ports::feedback_store::{
        FeedbackJournal, FeedbackStore, FeedbackStoreError, FeedbackStoreErrorKind,
        FeedbackStoreFuture, FeedbackTransaction,
    },
};

mod files;
mod validation;

// Different owner fixtures share the same bounded production worker pool.
// Serialize fixtures, while explicit concurrency tests still exercise admission.
#[cfg(test)]
pub(crate) static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Bootstrap-injected journal adapter; constructing it does not create files.
#[derive(Clone)]
pub struct FileFeedbackStore {
    runtime: RuntimePaths,
}

impl FileFeedbackStore {
    /// Captures resolved paths; OS security and disk access occur during `begin`.
    pub fn new(runtime: &RuntimePaths) -> Self {
        Self {
            runtime: runtime.clone(),
        }
    }
}

impl FeedbackStore for FileFeedbackStore {
    fn begin(&self) -> FeedbackStoreFuture<'_, Box<dyn FeedbackTransaction>> {
        Box::pin(async move {
            static ADMISSION: OnceLock<Arc<Semaphore>> = OnceLock::new();
            let permit = ADMISSION
                .get_or_init(|| Arc::new(Semaphore::new(4)))
                .clone()
                .try_acquire_owned()
                .map_err(|_| {
                    error(
                        FeedbackStoreErrorKind::Busy,
                        "feedback storage worker budget is occupied; retry later",
                    )
                })?;
            self.runtime
                .ensure_storage_access(StorageDirectoryAccess::OpenOrCreate)
                .await
                .map_err(|_| {
                    error(
                        FeedbackStoreErrorKind::Io,
                        "feedback data directory does not satisfy storage access policy",
                    )
                })?;
            let paths = self.runtime.feedback_store_paths();
            tokio::task::spawn_blocking(move || {
                let files = files::LockedFiles::open(paths, permit)?;
                let snapshot = files.read()?;
                let committed = Arc::new(snapshot.clone());
                Ok(Box::new(FileFeedbackTransaction {
                    files: Arc::new(files),
                    origin: committed.clone(),
                    committed,
                    snapshot,
                    poisoned: false,
                }) as Box<dyn FeedbackTransaction>)
            })
            .await
            .map_err(|_| {
                error(
                    FeedbackStoreErrorKind::Io,
                    "feedback storage worker did not complete",
                )
            })?
        })
    }
}

struct FileFeedbackTransaction {
    files: Arc<files::LockedFiles>,
    committed: Arc<FeedbackJournal>,
    origin: Arc<FeedbackJournal>,
    snapshot: FeedbackJournal,
    poisoned: bool,
}

impl FeedbackTransaction for FileFeedbackTransaction {
    fn snapshot(&self) -> &FeedbackJournal {
        &self.snapshot
    }

    fn snapshot_mut(&mut self) -> &mut FeedbackJournal {
        &mut self.snapshot
    }

    fn commit(&mut self) -> FeedbackStoreFuture<'_, ()> {
        Box::pin(async move {
            if self.poisoned {
                return Err(error(
                    FeedbackStoreErrorKind::InvalidData,
                    "feedback transaction was interrupted; reopen before continuing",
                ));
            }
            self.poisoned = true;
            let snapshot = std::mem::take(&mut self.snapshot);
            let files = self.files.clone();
            let committed = self.committed.clone();
            let origin = self.origin.clone();
            let (snapshot, result) = tokio::task::spawn_blocking(move || {
                let result = validation::validate_transition(&committed, &snapshot)
                    .and_then(|()| validation::validate_recovery(&origin, &snapshot))
                    .and_then(|()| files.write(&snapshot));
                let persisted = result.as_ref().ok().map(|()| Arc::new(snapshot.clone()));
                // Release the worker's ownership before the join result becomes
                // observable; cancellation still retains the lock until disk I/O ends.
                drop(files);
                (snapshot, (result, persisted))
            })
            .await
            .map_err(|_| {
                error(
                    FeedbackStoreErrorKind::Io,
                    "feedback commit worker did not complete; reopen to recover",
                )
            })?;
            self.snapshot = snapshot;
            if let Some(committed) = result.1 {
                self.committed = committed;
            }
            // Any failed write may already have reached disk; reopen rather than
            // allowing a caller to overwrite a potentially committed send claim.
            self.poisoned = result.0.is_err();
            result.0
        })
    }
}

fn error(kind: FeedbackStoreErrorKind, message: &str) -> FeedbackStoreError {
    FeedbackStoreError {
        kind,
        message: message.to_owned(),
    }
}

fn io_error(error: std::io::Error) -> FeedbackStoreError {
    // Only the error category is retained: OS messages may expose private paths.
    FeedbackStoreError {
        kind: FeedbackStoreErrorKind::Io,
        message: format!(
            "feedback journal I/O failed ({:?}); retry after checking local storage",
            error.kind()
        ),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
