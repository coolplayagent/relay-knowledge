//! Enforce managed Windows permissions immediately before a new pathname open.

use crate::{
    paths::{StorageDirectoryAccess, managed_database_validation},
    storage::StorageError,
};
use std::{
    path::Path,
    sync::{Condvar, Mutex},
    time::Duration,
};

const MAX_SECURITY_WAITERS: usize = 16;
const SECURITY_ADMISSION_TIMEOUT: Duration = Duration::from_secs(11);
static SECURITY_WORKER: SecurityGate = SecurityGate {
    state: Mutex::new(AdmissionState {
        active: false,
        waiting: 0,
    }),
    available: Condvar::new(),
};

struct AdmissionState {
    active: bool,
    waiting: usize,
}

struct SecurityGate {
    state: Mutex<AdmissionState>,
    available: Condvar,
}

struct SecurityPermit<'a>(&'a SecurityGate);

impl SecurityGate {
    fn acquire(&self, timeout: Duration) -> Result<SecurityPermit<'_>, StorageError> {
        let mut state = self.state.lock().map_err(|_| StorageError::LockPoisoned)?;
        if state.active {
            if state.waiting == MAX_SECURITY_WAITERS {
                return Err(StorageError::Busy(
                    "Windows security admission queue is full".to_owned(),
                ));
            }
            state.waiting += 1;
            let (updated, _) = self
                .available
                .wait_timeout_while(state, timeout, |state| state.active)
                .map_err(|_| StorageError::LockPoisoned)?;
            state = updated;
            state.waiting -= 1;
            if state.active {
                return Err(StorageError::Busy(
                    "Windows security admission timed out".to_owned(),
                ));
            }
        }
        state.active = true;
        Ok(SecurityPermit(self))
    }
}

impl Drop for SecurityPermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.active = false;
            self.0.available.notify_one();
        }
    }
}

pub(in crate::storage) fn validate_new_database_access(
    path: &Path,
    access: StorageDirectoryAccess,
) -> Result<(), StorageError> {
    let validation = managed_database_validation(path, access)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    if let Some(validation) = validation {
        // Wait only on an explicit synchronous/SQLite worker. A bounded queue
        // admits ordinary overlap without spawning extra security processes.
        let _admission = SECURITY_WORKER.acquire(SECURITY_ADMISSION_TIMEOUT)?;
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
