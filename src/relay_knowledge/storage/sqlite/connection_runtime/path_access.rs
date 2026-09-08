//! Enforce managed Windows permissions immediately before a new pathname open.

use crate::{paths::managed_database_validation, storage::StorageError};
use std::path::Path;

pub(in crate::storage) fn validate_new_database_access(path: &Path) -> Result<(), StorageError> {
    let validation = managed_database_validation(path)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    if let Some(validation) = validation {
        // Fresh catalog/diagnostic connections and import attachments already
        // execute on explicit SQLite workers; never block an async executor.
        tokio::runtime::Handle::try_current()
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?
            .block_on(validation)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "path_access_tests.rs"]
mod tests;
