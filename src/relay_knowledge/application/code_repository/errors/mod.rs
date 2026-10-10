//! Maps code-repository storage boundary failures into application API errors.

use crate::{api::ApiError, storage::StorageError};

pub(super) fn storage_api_error(error: StorageError) -> ApiError {
    match error {
        StorageError::CapacityExceeded(message) => ApiError::qos_rejected(message),
        StorageError::ResourceBudgetExceeded(message) => {
            ApiError::resource_budget_exceeded(message)
        }
        StorageError::Invariant(message) => ApiError::internal(message),
        other => ApiError::storage_unavailable(other.to_string()),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
