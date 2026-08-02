//! Shared MCP method error mapping across runtime handlers.

use crate::{
    api::{ApiError, ErrorKind},
    interfaces::agent::AgentAdapterError,
};

pub(in crate::interfaces::agent::mcp) struct McpMethodError {
    pub(in crate::interfaces::agent::mcp) code: i64,
    pub(in crate::interfaces::agent::mcp) kind: &'static str,
    pub(in crate::interfaces::agent::mcp) message: String,
}

impl McpMethodError {
    pub(in crate::interfaces::agent::mcp) fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            kind: "invalid_argument",
            message: message.into(),
        }
    }

    pub(in crate::interfaces::agent::mcp) fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            kind: "internal",
            message: message.into(),
        }
    }

    pub(in crate::interfaces::agent::mcp) fn timeout(message: impl Into<String>) -> Self {
        Self {
            code: -32000,
            kind: "timeout",
            message: message.into(),
        }
    }

    pub(in crate::interfaces::agent::mcp) fn api(error: ApiError) -> Self {
        Self {
            code: -32000,
            kind: match error.error_kind {
                ErrorKind::InvalidArgument => "invalid_argument",
                ErrorKind::StorageUnavailable => "storage_unavailable",
                ErrorKind::QosRejected => "qos_rejected",
                ErrorKind::Timeout => "timeout",
                ErrorKind::Internal => "internal",
            },
            message: error.message,
        }
    }

    pub(in crate::interfaces::agent::mcp) fn adapter(error: AgentAdapterError) -> Self {
        Self {
            code: -32000,
            kind: error.kind.as_str(),
            message: error.message,
        }
    }
}

#[cfg(test)]
#[path = "method_error_tests.rs"]
mod tests;
