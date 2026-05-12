use std::{error::Error, fmt};

use crate::{api::AgentAccessPolicy, domain::SourceScope};

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
    let Some(scope) = scope else {
        return if policy.allow_unspecified_scope {
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
    let normalized = parsed.as_str().to_owned();

    if policy
        .allowed_scopes
        .iter()
        .any(|allowed| allowed == &normalized)
    {
        return Ok(Some(normalized));
    }

    Err(AgentAdapterError::new(
        AgentAdapterErrorKind::PermissionDenied,
        "source_scope is not authorized for this MCP policy",
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> AgentAccessPolicy {
        AgentAccessPolicy::new(vec!["docs".to_owned()], false, 10, 1024, 1000, false, false)
            .expect("policy should build")
    }

    #[test]
    fn rejects_missing_scope_when_policy_requires_one() {
        let error = authorize_scope(None, &policy()).expect_err("missing scope should fail");

        assert_eq!(error.kind, AgentAdapterErrorKind::InvalidScope);
    }

    #[test]
    fn authorizes_only_configured_scopes() {
        let allowed = authorize_scope(Some(" docs ".to_owned()), &policy())
            .expect("scope should be authorized");
        let denied =
            authorize_scope(Some("src".to_owned()), &policy()).expect_err("scope should be denied");

        assert_eq!(allowed.as_deref(), Some("docs"));
        assert_eq!(denied.kind, AgentAdapterErrorKind::PermissionDenied);
    }

    #[test]
    fn rejects_limits_above_policy_budget() {
        let error = authorize_limit(Some(11), &policy()).expect_err("limit should fail");

        assert_eq!(error.kind, AgentAdapterErrorKind::LimitExceeded);
    }
}
