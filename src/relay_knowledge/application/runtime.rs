use std::{error::Error, fmt, time::Duration};

use crate::{
    api::{AgentAccessPolicy, AgentPolicyError},
    env::{EnvError, EnvironmentConfig, RetrievalEnvOverrides},
    net::{NetworkConfig, NetworkConfigError, NetworkRuntime, NetworkRuntimeError},
    paths::{PathError, RuntimePaths},
    retrieval::{
        LOCAL_SEMANTIC_MODEL, LOCAL_VECTOR_DIMENSION, LOCAL_VECTOR_MODEL, ReadModelBackendConfig,
        ReadModelBackendMode, ReadModelBackendModeError, ReadModelMetadata,
    },
};

/// Resolved foundation configuration shared by all interfaces.
#[derive(Debug, Clone)]
pub struct RuntimeConfiguration {
    pub paths: RuntimePaths,
    pub network: NetworkRuntime,
    pub agent: AgentRuntimeConfig,
    pub retrieval: ReadModelBackendConfig,
}

impl RuntimeConfiguration {
    /// Resolves runtime configuration from the current process environment.
    pub async fn from_process_environment() -> Result<Self, RuntimeConfigurationError> {
        let environment =
            EnvironmentConfig::from_process().map_err(RuntimeConfigurationError::Environment)?;

        Self::from_environment(&environment).await
    }

    /// Resolves runtime configuration from a typed environment snapshot.
    pub async fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, RuntimeConfigurationError> {
        let network = NetworkConfig::from_overrides(&environment.network)
            .map_err(RuntimeConfigurationError::Network)?;
        let agent = AgentRuntimeConfig::from_environment(environment, network.http.request_timeout)
            .map_err(RuntimeConfigurationError::Agent)?;
        let retrieval = retrieval_config_from_environment(&environment.retrieval)
            .map_err(RuntimeConfigurationError::Retrieval)?;

        Ok(Self {
            paths: RuntimePaths::resolve(&environment.platform, &environment.paths)
                .map_err(RuntimeConfigurationError::Paths)?,
            network: NetworkRuntime::from_config(network),
            agent,
            retrieval,
        })
    }
}

/// Resident agent protocol runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRuntimeConfig {
    pub mcp_streamable_http_enabled: bool,
    pub mcp_endpoint: String,
    pub mcp_allowed_origins: Vec<String>,
    pub access_policy: AgentAccessPolicy,
}

impl AgentRuntimeConfig {
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
            environment.agent.mcp_allow_index_refresh.unwrap_or(false),
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

/// Error raised while composing foundational runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeConfigurationError {
    Environment(EnvError),
    Paths(PathError),
    Network(NetworkConfigError),
    NetworkRuntime(NetworkRuntimeError),
    Agent(AgentRuntimeConfigError),
    Retrieval(RetrievalRuntimeConfigError),
}

impl fmt::Display for RuntimeConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment(error) => write!(formatter, "{error}"),
            Self::Paths(error) => write!(formatter, "{error}"),
            Self::Network(error) => write!(formatter, "{error}"),
            Self::NetworkRuntime(error) => write!(formatter, "{error}"),
            Self::Agent(error) => write!(formatter, "{error}"),
            Self::Retrieval(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for RuntimeConfigurationError {}

/// Retrieval runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalRuntimeConfigError {
    InvalidBackend(ReadModelBackendModeError),
    DimensionTooLarge(usize),
}

impl fmt::Display for RetrievalRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBackend(error) => write!(formatter, "{error}"),
            Self::DimensionTooLarge(value) => {
                write!(formatter, "embedding dimension {value} does not fit in u32")
            }
        }
    }
}

impl Error for RetrievalRuntimeConfigError {}

fn retrieval_config_from_environment(
    overrides: &RetrievalEnvOverrides,
) -> Result<ReadModelBackendConfig, RetrievalRuntimeConfigError> {
    let dimension = match overrides.embedding_dimension {
        Some(value) => u32::try_from(value)
            .map_err(|_| RetrievalRuntimeConfigError::DimensionTooLarge(value))?,
        None => LOCAL_VECTOR_DIMENSION,
    };
    let text_model = overrides
        .text_embedding_model
        .clone()
        .unwrap_or_else(|| LOCAL_VECTOR_MODEL.to_owned());
    let semantic_model = overrides
        .text_embedding_model
        .clone()
        .unwrap_or_else(|| LOCAL_SEMANTIC_MODEL.to_owned());
    let image_model = overrides
        .image_embedding_model
        .clone()
        .unwrap_or_else(|| "relay-local-image-hash-v1".to_owned());

    Ok(ReadModelBackendConfig {
        semantic_mode: parse_backend_mode(overrides.semantic_backend.as_deref())?,
        vector_mode: parse_backend_mode(overrides.vector_backend.as_deref())?,
        semantic_model: ReadModelMetadata {
            name: semantic_model,
            dimension,
        },
        vector_model: ReadModelMetadata {
            name: text_model,
            dimension,
        },
        image_model: ReadModelMetadata {
            name: image_model,
            dimension,
        },
    })
}

fn parse_backend_mode(
    value: Option<&str>,
) -> Result<ReadModelBackendMode, RetrievalRuntimeConfigError> {
    value
        .map(ReadModelBackendMode::parse)
        .transpose()
        .map_err(RetrievalRuntimeConfigError::InvalidBackend)
        .map(|mode| mode.unwrap_or(ReadModelBackendMode::Local))
}

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
mod tests {
    use super::*;
    use crate::env::PlatformKind;

    #[tokio::test]
    async fn resolves_mcp_agent_runtime_from_environment() {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED", "true"),
                ("RELAY_KNOWLEDGE_MCP_ENDPOINT", "/relay-mcp"),
                (
                    "RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS",
                    "http://localhost:3000",
                ),
                ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs,src"),
                ("RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE", "true"),
                ("RELAY_KNOWLEDGE_MCP_MAX_LIMIT", "3"),
                ("RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES", "4096"),
                ("RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH", "true"),
                ("RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS", "true"),
            ],
        )
        .expect("environment should parse");

        let runtime = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect("runtime should compose");

        assert!(runtime.agent.mcp_streamable_http_enabled);
        assert_eq!(runtime.agent.mcp_endpoint, "/relay-mcp");
        assert_eq!(runtime.agent.mcp_allowed_origins, ["http://localhost:3000"]);
        assert_eq!(runtime.agent.access_policy.allowed_scopes, ["docs", "src"]);
        assert!(runtime.agent.access_policy.allow_unspecified_scope);
        assert_eq!(runtime.agent.access_policy.max_limit, 3);
        assert_eq!(runtime.agent.access_policy.max_context_bytes, 4096);
        assert!(runtime.agent.access_policy.allow_index_refresh);
        assert!(runtime.agent.access_policy.allow_remote_clients);
    }

    #[tokio::test]
    async fn resolves_retrieval_read_model_runtime_from_environment() {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "external"),
                ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "external"),
                ("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", "text-embed-3-small"),
                ("RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL", "clip-vit-b32"),
                ("RELAY_KNOWLEDGE_EMBEDDING_DIMENSION", "1536"),
            ],
        )
        .expect("environment should parse");

        let runtime = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect("runtime should compose");

        assert_eq!(
            runtime.retrieval.semantic_mode,
            ReadModelBackendMode::External
        );
        assert_eq!(
            runtime.retrieval.vector_mode,
            ReadModelBackendMode::External
        );
        assert_eq!(runtime.retrieval.vector_model.name, "text-embed-3-small");
        assert_eq!(runtime.retrieval.image_model.name, "clip-vit-b32");
        assert_eq!(runtime.retrieval.vector_model.dimension, 1536);
    }

    #[tokio::test]
    async fn rejects_invalid_mcp_endpoint() {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [("RELAY_KNOWLEDGE_MCP_ENDPOINT", "mcp")],
        )
        .expect("environment should parse");

        let error = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect_err("invalid endpoint should fail");

        assert!(matches!(
            error,
            RuntimeConfigurationError::Agent(AgentRuntimeConfigError::InvalidEndpoint(_))
        ));
    }
}
