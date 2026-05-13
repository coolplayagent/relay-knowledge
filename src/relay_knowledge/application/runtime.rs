use std::{error::Error, fmt, time::Duration};

use crate::{
    api::{AgentAccessPolicy, AgentPolicyError},
    domain::WorkerKind,
    env::{
        EnvError, EnvironmentConfig, RELAY_KNOWLEDGE_EMBEDDING_API_KEY,
        RELAY_KNOWLEDGE_EMBEDDING_BASE_URL, RELAY_KNOWLEDGE_EMBEDDING_DIMENSION,
        RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL, RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL,
        RetrievalEnvOverrides,
    },
    net::{NetworkConfig, NetworkConfigError, NetworkRuntime, NetworkRuntimeError},
    paths::{PathError, RuntimePaths},
    retrieval::{
        DEFAULT_EMBEDDING_BATCH_SIZE, DEFAULT_EMBEDDING_MAX_CONCURRENCY, DEFAULT_EMBEDDING_TIMEOUT,
        EmbeddingProviderKind, EmbeddingProviderKindError, LOCAL_SEMANTIC_MODEL,
        LOCAL_VECTOR_DIMENSION, LOCAL_VECTOR_MODEL, ReadModelBackendConfig, ReadModelBackendMode,
        ReadModelBackendModeError, ReadModelMetadata, RemoteEmbeddingConfig,
    },
};

/// Resolved foundation configuration shared by all interfaces.
#[derive(Debug, Clone)]
pub struct RuntimeConfiguration {
    pub paths: RuntimePaths,
    pub network: NetworkRuntime,
    pub agent: AgentRuntimeConfig,
    pub retrieval: ReadModelBackendConfig,
    pub workers: WorkerRuntimeConfig,
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
        let workers = WorkerRuntimeConfig::from_environment(environment)
            .map_err(RuntimeConfigurationError::Workers)?;

        Ok(Self {
            paths: RuntimePaths::resolve(&environment.platform, &environment.paths)
                .map_err(RuntimeConfigurationError::Paths)?,
            network: NetworkRuntime::from_config(network),
            agent,
            retrieval,
            workers,
        })
    }
}

/// External worker runtime configuration and deterministic fallback policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRuntimeConfig {
    pub embedding_endpoint: Option<String>,
    pub ocr_endpoint: Option<String>,
    pub vision_endpoint: Option<String>,
    pub extractor_endpoint: Option<String>,
    pub max_in_flight: usize,
    pub silent_updates_enabled: bool,
}

impl WorkerRuntimeConfig {
    pub const DEFAULT_MAX_IN_FLIGHT: usize = 2;

    /// Builds worker config from typed environment overrides.
    pub fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, WorkerRuntimeConfigError> {
        Ok(Self {
            embedding_endpoint: validate_worker_endpoint(
                environment.workers.embedding_endpoint.clone(),
            )?,
            ocr_endpoint: validate_worker_endpoint(environment.workers.ocr_endpoint.clone())?,
            vision_endpoint: validate_worker_endpoint(environment.workers.vision_endpoint.clone())?,
            extractor_endpoint: validate_worker_endpoint(
                environment.workers.extractor_endpoint.clone(),
            )?,
            max_in_flight: environment
                .workers
                .max_in_flight
                .unwrap_or(Self::DEFAULT_MAX_IN_FLIGHT),
            silent_updates_enabled: environment.workers.silent_updates_enabled.unwrap_or(false),
        })
    }

    /// Returns the configured endpoint for a worker kind.
    pub fn endpoint_for(&self, kind: WorkerKind) -> Option<&str> {
        match kind {
            WorkerKind::Embedding => self.embedding_endpoint.as_deref(),
            WorkerKind::Ocr => self.ocr_endpoint.as_deref(),
            WorkerKind::Vision => self.vision_endpoint.as_deref(),
            WorkerKind::Extractor => self.extractor_endpoint.as_deref(),
        }
    }
}

/// Worker runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRuntimeConfigError {
    InvalidEndpoint(String),
}

impl fmt::Display for WorkerRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint(value) => write!(
                formatter,
                "worker endpoint '{value}' must use http:// and include a host"
            ),
        }
    }
}

impl Error for WorkerRuntimeConfigError {}

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
    Workers(WorkerRuntimeConfigError),
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
            Self::Workers(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for RuntimeConfigurationError {}

/// Retrieval runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalRuntimeConfigError {
    InvalidBackend(ReadModelBackendModeError),
    InvalidProvider(EmbeddingProviderKindError),
    EmptyModelName(&'static str),
    MissingRemoteValue(&'static str),
    InvalidRemoteBaseUrl(String),
    DimensionTooLarge(usize),
}

impl fmt::Display for RetrievalRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBackend(error) => write!(formatter, "{error}"),
            Self::InvalidProvider(error) => write!(formatter, "{error}"),
            Self::EmptyModelName(variable) => {
                write!(formatter, "{variable} must not be blank")
            }
            Self::MissingRemoteValue(variable) => {
                write!(
                    formatter,
                    "{variable} is required when a read model backend is external"
                )
            }
            Self::InvalidRemoteBaseUrl(value) => {
                write!(
                    formatter,
                    "embedding base URL '{value}' must use http:// or https://"
                )
            }
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
    let semantic_mode = parse_backend_mode(overrides.semantic_backend.as_deref())?;
    let vector_mode = parse_backend_mode(overrides.vector_backend.as_deref())?;
    let remote_required = semantic_mode == ReadModelBackendMode::External
        || vector_mode == ReadModelBackendMode::External;
    require_remote_model_metadata(overrides, remote_required)?;
    let dimension = match overrides.embedding_dimension {
        Some(value) => u32::try_from(value)
            .map_err(|_| RetrievalRuntimeConfigError::DimensionTooLarge(value))?,
        None => LOCAL_VECTOR_DIMENSION,
    };
    let text_model = model_name_override(
        overrides.text_embedding_model.as_deref(),
        RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL,
        LOCAL_VECTOR_MODEL,
    )?;
    let semantic_model = model_name_override(
        overrides.text_embedding_model.as_deref(),
        RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL,
        LOCAL_SEMANTIC_MODEL,
    )?;
    let image_model = model_name_override(
        overrides.image_embedding_model.as_deref(),
        RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL,
        "relay-local-image-hash-v1",
    )?;

    let remote_embedding = remote_embedding_config_from_environment(overrides, remote_required)?;

    Ok(ReadModelBackendConfig {
        semantic_mode,
        vector_mode,
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
        remote_embedding,
    })
}

fn require_remote_model_metadata(
    overrides: &RetrievalEnvOverrides,
    required: bool,
) -> Result<(), RetrievalRuntimeConfigError> {
    if !required {
        return Ok(());
    }
    if overrides.text_embedding_model.is_none() {
        return Err(RetrievalRuntimeConfigError::MissingRemoteValue(
            RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL,
        ));
    }
    if overrides.embedding_dimension.is_none() {
        return Err(RetrievalRuntimeConfigError::MissingRemoteValue(
            RELAY_KNOWLEDGE_EMBEDDING_DIMENSION,
        ));
    }

    Ok(())
}

fn remote_embedding_config_from_environment(
    overrides: &RetrievalEnvOverrides,
    required: bool,
) -> Result<Option<RemoteEmbeddingConfig>, RetrievalRuntimeConfigError> {
    if !required {
        return Ok(None);
    }
    let provider = overrides
        .llm_provider
        .as_deref()
        .map(EmbeddingProviderKind::parse)
        .transpose()
        .map_err(RetrievalRuntimeConfigError::InvalidProvider)?
        .unwrap_or(EmbeddingProviderKind::OpenAiCompatible);
    let base_url = required_remote_value(
        overrides.embedding_base_url.as_deref(),
        RELAY_KNOWLEDGE_EMBEDDING_BASE_URL,
    )?;
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err(RetrievalRuntimeConfigError::InvalidRemoteBaseUrl(base_url));
    }
    let api_key = required_remote_value(
        overrides.embedding_api_key.as_deref(),
        RELAY_KNOWLEDGE_EMBEDDING_API_KEY,
    )?;
    let batch_size = overrides
        .embedding_batch_size
        .unwrap_or(DEFAULT_EMBEDDING_BATCH_SIZE);
    let timeout = overrides
        .embedding_timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_EMBEDDING_TIMEOUT);
    let max_concurrency = overrides
        .embedding_max_concurrency
        .unwrap_or(DEFAULT_EMBEDDING_MAX_CONCURRENCY);

    Ok(Some(RemoteEmbeddingConfig {
        provider,
        base_url,
        api_key,
        batch_size,
        timeout,
        max_concurrency,
    }))
}

fn required_remote_value(
    value: Option<&str>,
    variable: &'static str,
) -> Result<String, RetrievalRuntimeConfigError> {
    match value.map(str::trim) {
        Some(trimmed) if !trimmed.is_empty() => Ok(trimmed.to_owned()),
        _ => Err(RetrievalRuntimeConfigError::MissingRemoteValue(variable)),
    }
}

fn model_name_override(
    value: Option<&str>,
    variable: &'static str,
    default: &'static str,
) -> Result<String, RetrievalRuntimeConfigError> {
    match value {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Err(RetrievalRuntimeConfigError::EmptyModelName(variable))
            } else {
                Ok(trimmed.to_owned())
            }
        }
        None => Ok(default.to_owned()),
    }
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

fn validate_worker_endpoint(
    value: Option<String>,
) -> Result<Option<String>, WorkerRuntimeConfigError> {
    value
        .map(|endpoint| {
            let trimmed = endpoint.trim();
            if is_valid_worker_http_endpoint(trimmed) {
                Ok(trimmed.to_owned())
            } else {
                Err(WorkerRuntimeConfigError::InvalidEndpoint(endpoint))
            }
        })
        .transpose()
}

fn is_valid_worker_http_endpoint(value: &str) -> bool {
    let Some(remainder) = value.strip_prefix("http://") else {
        return false;
    };
    let authority = remainder
        .split_once('/')
        .map_or(remainder, |(authority, _)| authority);
    if authority.is_empty() || authority.contains(char::is_whitespace) {
        return false;
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        return !host.is_empty() && port.parse::<u16>().is_ok_and(|port| port > 0);
    }

    !authority.is_empty()
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
                ("RELAY_KNOWLEDGE_LLM_PROVIDER", "openai_compatible"),
                (
                    "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
                    "https://embeddings.example/v1",
                ),
                ("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", "secret-key"),
                ("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", "text-embed-3-small"),
                ("RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL", "clip-vit-b32"),
                ("RELAY_KNOWLEDGE_EMBEDDING_DIMENSION", "1536"),
                ("RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE", "16"),
                ("RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS", "9000"),
                ("RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY", "2"),
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
        let remote = runtime
            .retrieval
            .remote_embedding
            .expect("remote embedding config should be present");
        assert_eq!(remote.provider, EmbeddingProviderKind::OpenAiCompatible);
        assert_eq!(remote.redacted_base_url(), "https://embeddings.example");
        assert_eq!(remote.batch_size, 16);
        assert_eq!(remote.timeout, Duration::from_millis(9000));
        assert_eq!(remote.max_concurrency, 2);
    }

    #[tokio::test]
    async fn rejects_external_backend_without_remote_model_metadata() {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "external"),
                (
                    "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
                    "https://embeddings.example/v1",
                ),
                ("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", "secret-key"),
            ],
        )
        .expect("environment should parse");

        let error = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect_err("external backend should require explicit model metadata");

        assert!(matches!(
            error,
            RuntimeConfigurationError::Retrieval(RetrievalRuntimeConfigError::MissingRemoteValue(
                RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL
            ))
        ));
    }

    #[tokio::test]
    async fn rejects_blank_retrieval_model_overrides() {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", "   ")],
        )
        .expect("environment should parse");

        let error = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect_err("blank model name should fail");

        assert!(matches!(
            error,
            RuntimeConfigurationError::Retrieval(RetrievalRuntimeConfigError::EmptyModelName(
                RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL
            ))
        ));
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

    #[tokio::test]
    async fn rejects_worker_endpoint_without_http_host() {
        for endpoint in ["https://worker.local", "http://", "http://:8792"] {
            let environment = EnvironmentConfig::from_pairs(
                PlatformKind::Unix,
                [("RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT", endpoint)],
            )
            .expect("environment should parse");

            let error = RuntimeConfiguration::from_environment(&environment)
                .await
                .expect_err("invalid worker endpoint should fail");

            assert!(matches!(
                error,
                RuntimeConfigurationError::Workers(WorkerRuntimeConfigError::InvalidEndpoint(_))
            ));
        }
    }
}
