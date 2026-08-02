use std::{error::Error, fmt};

use crate::{api::AgentAccessPolicy, domain::SourceScope};

pub const MAX_AGENT_QUERY_CHARS: usize = 10_000;
pub const MAX_AGENT_PATH_CHARS: usize = 4_096;

/// Stable adapter error categories for protocol-level governance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentAdapterErrorKind {
    PermissionDenied,
    InvalidScope,
    LimitExceeded,
    QosRejected,
    StorageUnavailable,
    Timeout,
    Cancelled,
    UnsupportedOperation,
    InvalidArgument,
    Internal,
}

impl AgentAdapterErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PermissionDenied => "permission_denied",
            Self::InvalidScope => "invalid_scope",
            Self::LimitExceeded => "limit_exceeded",
            Self::QosRejected => "qos_rejected",
            Self::StorageUnavailable => "storage_unavailable",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::UnsupportedOperation => "unsupported_operation",
            Self::InvalidArgument => "invalid_argument",
            Self::Internal => "internal",
        }
    }
}

/// Error raised before or during agent adapter request mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAdapterError {
    pub kind: AgentAdapterErrorKind,
    pub message: String,
}

impl AgentAdapterError {
    pub fn new(kind: AgentAdapterErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for AgentAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl Error for AgentAdapterError {}

/// Validates and authorizes an optional source scope before service invocation.
pub fn authorize_scope(
    scope: Option<String>,
    policy: &AgentAccessPolicy,
) -> Result<Option<String>, AgentAdapterError> {
    let Some(normalized) = normalize_scope_for_policy(scope, policy.allow_unspecified_scope)?
    else {
        return Ok(None);
    };

    if policy
        .allowed_scopes
        .iter()
        .any(|allowed| allowed == &normalized)
    {
        return Ok(Some(normalized));
    }

    Err(scope_not_authorized(&normalized))
}

/// Normalizes optional source scope input while preserving policy semantics.
pub fn normalize_scope_for_policy(
    scope: Option<String>,
    allow_unspecified_scope: bool,
) -> Result<Option<String>, AgentAdapterError> {
    let Some(scope) = scope else {
        return if allow_unspecified_scope {
            Ok(None)
        } else {
            Err(AgentAdapterError::new(
                AgentAdapterErrorKind::InvalidScope,
                "source_scope is required by the MCP access policy",
            ))
        };
    };
    let parsed = SourceScope::parse(scope).map_err(|error| {
        AgentAdapterError::new(AgentAdapterErrorKind::InvalidScope, error.to_string())
    })?;

    Ok(Some(parsed.as_str().to_owned()))
}

/// Builds the shared scope authorization denial.
pub fn scope_not_authorized(scope: &str) -> AgentAdapterError {
    AgentAdapterError::new(
        AgentAdapterErrorKind::PermissionDenied,
        format!("source_scope '{scope}' is not authorized for this agent access policy"),
    )
}

/// Validates tool limit without silently expanding caller budgets.
pub fn authorize_limit(
    limit: Option<usize>,
    policy: &AgentAccessPolicy,
) -> Result<usize, AgentAdapterError> {
    let limit = limit.unwrap_or(policy.max_limit);
    if limit == 0 {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            "limit must be greater than zero",
        ));
    }
    if limit > policy.max_limit {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::LimitExceeded,
            format!("limit {limit} exceeds MCP max_limit {}", policy.max_limit),
        ));
    }

    Ok(limit)
}

pub fn validate_query_text(field: &str, value: &str) -> Result<(), AgentAdapterError> {
    validate_text_length(field, value, MAX_AGENT_QUERY_CHARS, "query text")
}

pub fn validate_optional_query_text(
    field: &str,
    value: Option<&str>,
) -> Result<(), AgentAdapterError> {
    if let Some(value) = value {
        validate_query_text(field, value)?;
    }

    Ok(())
}

pub fn validate_path_text(field: &str, value: &str) -> Result<(), AgentAdapterError> {
    validate_text_length(field, value, MAX_AGENT_PATH_CHARS, "path input")
}

pub fn validate_path_texts(field: &str, values: &[String]) -> Result<(), AgentAdapterError> {
    for value in values {
        validate_path_text(field, value)?;
    }

    Ok(())
}

fn validate_text_length(
    field: &str,
    value: &str,
    max_chars: usize,
    label: &str,
) -> Result<(), AgentAdapterError> {
    let count = value.chars().count();
    if count > max_chars {
        return Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("{field} {label} exceeds {max_chars} characters"),
        ));
    }

    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
