//! Enforce managed Windows permissions immediately before a new pathname open.

use crate::{paths::managed_database_validation, storage::StorageError};
use std::{path::Path, sync::Mutex};

static SECURITY_WORKER: Mutex<()> = Mutex::new(());

pub(in crate::storage) fn validate_new_database_access(path: &Path) -> Result<(), StorageError> {
    let validation = managed_database_validation(path)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    if let Some(validation) = validation {
        // Synchronous callers need no ambient runtime. At most one scoped
        // worker/child is active; concurrent admission fails observably.
        let _admission = SECURITY_WORKER.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => {
                StorageError::Busy("Windows storage security worker is occupied".to_owned())
            }
            std::sync::TryLockError::Poisoned(_) => StorageError::LockPoisoned,
        })?;
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("sqlite-security-check".to_owned())
                .spawn_scoped(scope, move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    runtime
                        .block_on(validation)
                        .map_err(|error| StorageError::InvalidInput(error.to_string()))
                })?
                .join()
                .map_err(|_| {
                    StorageError::InvalidInput(
                        "Windows storage security worker panicked".to_owned(),
                    )
                })?
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "path_access_tests.rs"]
mod tests;
