use serde::{Deserialize, Serialize};

use super::ApiMetadata;

/// Stable error categories used across interface adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidArgument,
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
}
