use std::{error::Error, fmt, time::Duration};

use crate::{
    api::{AgentAccessPolicy, AgentPolicyError},
    env::EnvironmentConfig,
};

/// Resident agent protocol runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRuntimeConfig {
    pub mcp_streamable_http_enabled: bool,
    pub mcp_endpoint: String,
    pub mcp_allowed_origins: Vec<String>,
    pub access_policy: AgentAccessPolicy,
    pub audit_sink_enabled: bool,
    pub audit_queue_depth: usize,
}

impl AgentRuntimeConfig {
    pub const DEFAULT_AUDIT_QUEUE_DEPTH: usize = 1024;

    /// Builds agent protocol config from typed environment overrides.
    pub fn from_environment(
        environment: &EnvironmentConfig,
        request_timeout: Duration,
    ) -> Result<Self, AgentRuntimeConfigError> {
        let max_runtime_ms = agent_runtime_budget_ms(request_timeout);
        let access_policy = AgentAccessPolicy::new(
            split_csv(environment.agent.mcp_allowed_scopes.as_deref())?,
            environment
                .agent
                .mcp_allow_unspecified_scope
                .unwrap_or(false),
            environment
                .agent
                .mcp_max_limit
                .unwrap_or(AgentAccessPolicy::DEFAULT_MAX_LIMIT),
            environment
                .agent
                .mcp_max_context_bytes
                .unwrap_or(AgentAccessPolicy::DEFAULT_MAX_CONTEXT_BYTES),
            max_runtime_ms,
            environment.agent.mcp_allow_remote_clients.unwrap_or(false),
        )
        .map_err(AgentRuntimeConfigError::Policy)?;

        Ok(Self {
            mcp_streamable_http_enabled: environment
                .agent
                .mcp_streamable_http_enabled
                .unwrap_or(false),
            mcp_endpoint: validate_endpoint(
                environment.agent.mcp_endpoint.as_deref().unwrap_or("/mcp"),
            )?,
            mcp_allowed_origins: split_csv(environment.agent.mcp_allowed_origins.as_deref())?,
            access_policy,
            audit_sink_enabled: environment.agent.audit_sink_enabled.unwrap_or(false),
            audit_queue_depth: environment
                .agent
                .audit_queue_depth
                .unwrap_or(Self::DEFAULT_AUDIT_QUEUE_DEPTH),
        })
    }

    /// Returns a copy with streamable HTTP forced on by a CLI command.
    pub fn with_streamable_http_enabled(mut self) -> Self {
        self.mcp_streamable_http_enabled = true;
        self
    }
}

/// Agent runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimeConfigError {
    InvalidEndpoint(String),
    EmptyListValue,
    Policy(AgentPolicyError),
}

impl fmt::Display for AgentRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint(value) => {
                write!(
                    formatter,
                    "MCP endpoint '{value}' must be an absolute HTTP path"
                )
            }
            Self::EmptyListValue => {
                write!(formatter, "MCP comma-separated values must not be empty")
            }
            Self::Policy(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for AgentRuntimeConfigError {}

fn validate_endpoint(value: &str) -> Result<String, AgentRuntimeConfigError> {
    let trimmed = value.trim();
    if !trimmed.starts_with('/')
        || trimmed.contains(char::is_whitespace)
        || trimmed.contains('?')
        || trimmed.contains('#')
    {
        return Err(AgentRuntimeConfigError::InvalidEndpoint(value.to_owned()));
    }

    Ok(trimmed.to_owned())
}

fn split_csv(value: Option<&str>) -> Result<Vec<String>, AgentRuntimeConfigError> {
    value
        .map(|items| {
            items
                .split(',')
                .map(str::trim)
                .map(|item| {
                    if item.is_empty() {
                        Err(AgentRuntimeConfigError::EmptyListValue)
                    } else {
                        Ok(item.to_owned())
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn agent_runtime_budget_ms(request_timeout: Duration) -> u64 {
    let budget = request_timeout.saturating_sub(Duration::from_millis(1));
    duration_millis(budget).max(1)
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod agent_tests;
