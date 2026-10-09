//! Runtime ownership of feedback journal and publication-lock locations.

use std::path::PathBuf;

use crate::project::{
    FEEDBACK_DIRECTORY_NAME, FEEDBACK_JOURNAL_FILE_NAME, FEEDBACK_LOCK_FILE_NAME,
    FEEDBACK_PREPARED_FILE_NAME,
};

use super::RuntimePaths;

/// Co-located journal paths required for atomic publication on one filesystem.
#[derive(Debug, Clone)]
pub struct FeedbackStorePaths {
    pub directory: PathBuf,
    pub journal: PathBuf,
    pub lock: PathBuf,
    pub prepared: PathBuf,
}

impl RuntimePaths {
    /// Resolves private feedback state separately from graph facts and caches.
    pub fn feedback_store_paths(&self) -> FeedbackStorePaths {
        let directory = self.data_dir.join(FEEDBACK_DIRECTORY_NAME);
        FeedbackStorePaths {
            journal: directory.join(FEEDBACK_JOURNAL_FILE_NAME),
            lock: directory.join(FEEDBACK_LOCK_FILE_NAME),
            prepared: directory.join(FEEDBACK_PREPARED_FILE_NAME),
            directory,
        }
    }
}

#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
