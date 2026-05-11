use serde::{Deserialize, Serialize};

use super::ApiMetadata;

/// Stable error categories used across interface adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidArgument,
    StorageUnavailable,
    Timeout,
    Internal,
}

/// API error shape suitable for JSON and streaming output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error_kind: ErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ApiMetadata>,
}

impl ApiError {
    /// Creates an invalid argument error.
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self {
            error_kind: ErrorKind::InvalidArgument,
            message: message.into(),
            metadata: None,
        }
    }

    /// Creates a storage boundary error without exposing backend internals.
    pub fn storage_unavailable(message: impl Into<String>) -> Self {
        Self {
            error_kind: ErrorKind::StorageUnavailable,
            message: message.into(),
            metadata: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_stable_error_shapes() {
        let invalid = ApiError::invalid_argument("bad input");
        let storage = ApiError::storage_unavailable("database busy");

        assert_eq!(invalid.error_kind, ErrorKind::InvalidArgument);
        assert_eq!(invalid.message, "bad input");
        assert_eq!(storage.error_kind, ErrorKind::StorageUnavailable);
        assert_eq!(storage.message, "database busy");
    }
}
