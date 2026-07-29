//! Maps storage boundary failures into application API errors.

use crate::{api::ApiError, storage::StorageError};

pub(super) fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}
