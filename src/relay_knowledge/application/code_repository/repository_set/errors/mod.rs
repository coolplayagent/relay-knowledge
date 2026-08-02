//! Maps repository-set storage failures into stable application errors.

use crate::{api::ApiError, storage::StorageError};

pub(super) fn storage_api_error(error: StorageError) -> ApiError {
    match error {
        StorageError::InvalidInput(message) => ApiError::invalid_argument(message),
        other => ApiError::storage_unavailable(other.to_string()),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
