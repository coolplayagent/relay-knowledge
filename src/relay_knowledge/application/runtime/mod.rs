use std::{
    error::Error,
    fmt,
    path::{Component, PathBuf},
    time::Duration,
};

use crate::{
    api::{AgentAccessPolicy, AgentPolicyError},
    domain::{RerankMode, RerankModeError, WorkerKind},
    env::{
        EnvError, EnvironmentConfig, PlatformKind, RELAY_KNOWLEDGE_EMBEDDING_API_KEY,
        RELAY_KNOWLEDGE_EMBEDDING_BASE_URL, RELAY_KNOWLEDGE_EMBEDDING_DIMENSION,
        RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL, RELAY_KNOWLEDGE_RERANK_MODEL,
        RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL, RetrievalEnvOverrides,
    },
    net::{NetworkConfig, NetworkConfigError, NetworkRuntime, NetworkRuntimeError},
    observability::{ObservabilityRuntime, TelemetryConfig},
    paths::{PathError, RuntimePaths, default_user_document_roots},
    retrieval::{
        DEFAULT_EMBEDDING_BATCH_SIZE, DEFAULT_EMBEDDING_MAX_CONCURRENCY, DEFAULT_EMBEDDING_TIMEOUT,
        DEFAULT_RERANK_CANDIDATE_MULTIPLIER, DEFAULT_RERANK_MAX_CANDIDATES, DEFAULT_RERANK_TIMEOUT,
        EmbeddingProviderKind, EmbeddingProviderKindError, LOCAL_RERANK_MODEL,
        LOCAL_SEMANTIC_MODEL, LOCAL_VECTOR_DIMENSION, LOCAL_VECTOR_MODEL, ReadModelBackendConfig,
        ReadModelBackendMode, ReadModelBackendModeError, ReadModelMetadata, RemoteEmbeddingConfig,
        RerankConfig,
    },
    storage::StorageTopology,
};

use super::update::{UpdateRuntimeConfig, UpdateRuntimeConfigError};

/// Resolved foundation configuration shared by all interfaces.
#[derive(Debug, Clone)]
pub struct RuntimeConfiguration {
    pub paths: RuntimePaths,
    pub network: NetworkRuntime,
    pub observability: ObservabilityRuntime,
    pub agent: AgentRuntimeConfig,
    pub retrieval: ReadModelBackendConfig,
    pub workers: WorkerRuntimeConfig,
    pub file_index: FileIndexRuntimeConfig,
    pub updates: UpdateRuntimeConfig,
    pub storage: StorageRuntimeConfig,
    pub watcher: crate::watcher::WatcherConfig,
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
        let observability =
            ObservabilityRuntime::new(TelemetryConfig::from_environment(&environment.telemetry));
        let agent = AgentRuntimeConfig::from_environment(environment, network.http.request_timeout)
            .map_err(RuntimeConfigurationError::Agent)?;
        let retrieval = retrieval_config_from_environment(&environment.retrieval)
            .map_err(RuntimeConfigurationError::Retrieval)?;
        let workers = WorkerRuntimeConfig::from_environment(environment)
            .map_err(RuntimeConfigurationError::Workers)?;
        let file_index = FileIndexRuntimeConfig::from_environment(environment)
            .map_err(RuntimeConfigurationError::FileIndex)?;
        let updates = UpdateRuntimeConfig::from_environment(&environment.updates)
            .map_err(RuntimeConfigurationError::Updates)?;
        let storage = StorageRuntimeConfig::from_environment(environment)
            .map_err(RuntimeConfigurationError::Storage)?;

        let watcher = crate::watcher::WatcherConfig::from_environment(&environment.watcher);

        Ok(Self {
            paths: RuntimePaths::resolve(&environment.platform, &environment.paths)
                .map_err(RuntimeConfigurationError::Paths)?,
            network: NetworkRuntime::from_config(network),
            observability,
            agent,
            retrieval,
            workers,
            file_index,
            updates,
            storage,
            watcher,
        })
    }
}

/// Storage backend topology selected for this runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageRuntimeConfig {
    pub topology: StorageTopology,
}

impl StorageRuntimeConfig {
    pub fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, StorageRuntimeConfigError> {
        let topology = environment
            .storage_topology
            .as_deref()
            .map(parse_storage_topology)
            .transpose()?
            .unwrap_or(StorageTopology::SingleSqlite);

        Ok(Self { topology })
    }
}

/// Storage runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageRuntimeConfigError {
    InvalidTopology(String),
}

impl fmt::Display for StorageRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTopology(value) => write!(
                formatter,
                "storage topology '{value}' must be single_sqlite or partitioned_sqlite"
            ),
        }
    }
}

impl Error for StorageRuntimeConfigError {}

fn parse_storage_topology(value: &str) -> Result<StorageTopology, StorageRuntimeConfigError> {
    match StorageTopology::parse(value) {
        Ok(topology) => Ok(topology),
        Err(_) => Err(StorageRuntimeConfigError::InvalidTopology(value.to_owned())),
    }
}

/// Runtime budgets and authorized roots for local file-location indexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexRuntimeConfig {
    pub enabled: bool,
    pub roots: Vec<FileIndexRootConfig>,
    pub excludes: Vec<String>,
    pub max_depth: usize,
    pub max_file_bytes: u64,
    pub scan_interval: Duration,
    pub scan_timeout: Duration,
    pub max_files_per_root: usize,
    pub query_timeout: Duration,
}

impl FileIndexRuntimeConfig {
    pub const DEFAULT_MAX_DEPTH: usize = 32;
    pub const DEFAULT_MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
    pub const DEFAULT_SCAN_INTERVAL: Duration = Duration::from_secs(900);
    pub const DEFAULT_SCAN_TIMEOUT: Duration = Duration::from_secs(300);
    pub const DEFAULT_MAX_FILES_PER_ROOT: usize = 50_000;
    pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_millis(750);

    pub fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, FileIndexRuntimeConfigError> {
        let mut roots = default_user_document_roots(&environment.platform)
            .map_err(FileIndexRuntimeConfigError::Paths)?
            .into_iter()
            .map(|path| FileIndexRootConfig::new("user-documents", path))
            .collect::<Vec<_>>();
        for root in split_semicolon(environment.file_index.roots.as_deref())? {
            roots.push(file_index_root_from_environment(
                "local-files",
                root,
                environment.platform.platform,
            )?);
        }
        roots.sort_by(|left, right| {
            left.scope_id
                .cmp(&right.scope_id)
                .then(left.root_id.cmp(&right.root_id))
        });
        roots.dedup_by(|left, right| {
            left.scope_id == right.scope_id && left.root_id == right.root_id
        });

        Ok(Self {
            enabled: environment.file_index.enabled.unwrap_or(false),
            roots,
            excludes: split_semicolon(environment.file_index.excludes.as_deref())?,
            max_depth: environment
                .file_index
                .max_depth
                .unwrap_or(Self::DEFAULT_MAX_DEPTH),
            max_file_bytes: environment
                .file_index
                .max_file_bytes
                .unwrap_or(Self::DEFAULT_MAX_FILE_BYTES),
            scan_interval: Duration::from_millis(
                environment
                    .file_index
                    .scan_interval_ms
                    .unwrap_or(duration_millis(Self::DEFAULT_SCAN_INTERVAL)),
            ),
            scan_timeout: Duration::from_millis(
                environment
                    .file_index
                    .scan_timeout_ms
                    .unwrap_or(duration_millis(Self::DEFAULT_SCAN_TIMEOUT)),
            ),
            max_files_per_root: environment
                .file_index
                .max_files_per_root
                .unwrap_or(Self::DEFAULT_MAX_FILES_PER_ROOT),
            query_timeout: Duration::from_millis(
                environment
                    .file_index
                    .query_timeout_ms
                    .unwrap_or(duration_millis(Self::DEFAULT_QUERY_TIMEOUT)),
            ),
        })
    }
}

/// One authorized local file index root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexRootConfig {
    pub scope_id: String,
    pub root_id: String,
    pub root_path: PathBuf,
}

impl FileIndexRootConfig {
    pub fn new(scope_id: impl Into<String>, root_path: PathBuf) -> Self {
        let root_path = normalize_file_index_root_path(root_path);
        let root_id = format!(
            "root-{:016x}",
            stable_hash64(root_path.to_string_lossy().as_bytes())
        );

        Self {
            scope_id: scope_id.into(),
            root_id,
            root_path,
        }
    }
}

fn normalize_file_index_root_path(root_path: PathBuf) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(&root_path) {
        return canonical;
    }

    let mut normalized = PathBuf::new();
    for component in root_path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => normalized.push(".."),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(value) => normalized.push(value),
        }
    }

    if normalized.as_os_str().is_empty() {
        root_path
    } else {
        normalized
    }
}

/// File index runtime validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileIndexRuntimeConfigError {
    EmptyListValue,
    RelativeRoot(String),
    Paths(PathError),
}

impl fmt::Display for FileIndexRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyListValue => {
                write!(formatter, "file index lists must not contain empty values")
            }
            Self::RelativeRoot(path) => {
                write!(
                    formatter,
                    "file index root '{path}' must be an absolute path"
                )
            }
            Self::Paths(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for FileIndexRuntimeConfigError {}

/// External worker runtime configuration and deterministic fallback policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRuntimeConfig {
    pub embedding_endpoint: Option<String>,
    pub ocr_endpoint: Option<String>,
    pub vision_endpoint: Option<String>,
    pub extractor_endpoint: Option<String>,
    pub max_in_flight: usize,
    pub code_index_max_in_flight: usize,
    pub silent_updates_enabled: bool,
}

impl WorkerRuntimeConfig {
    pub const DEFAULT_MAX_IN_FLIGHT: usize = 2;
    pub const DEFAULT_CODE_INDEX_MAX_IN_FLIGHT: usize = 2;
    pub const MAX_CODE_INDEX_MAX_IN_FLIGHT: usize = 8;

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
            code_index_max_in_flight: environment
                .workers
                .code_index_max_in_flight
                .unwrap_or(Self::DEFAULT_CODE_INDEX_MAX_IN_FLIGHT)
                .min(Self::MAX_CODE_INDEX_MAX_IN_FLIGHT),
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
    FileIndex(FileIndexRuntimeConfigError),
    Updates(UpdateRuntimeConfigError),
    Storage(StorageRuntimeConfigError),
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
            Self::FileIndex(error) => write!(formatter, "{error}"),
            Self::Updates(error) => write!(formatter, "{error}"),
            Self::Storage(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for RuntimeConfigurationError {}

/// Retrieval runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalRuntimeConfigError {
    InvalidBackend(ReadModelBackendModeError),
    InvalidRerankBackend(RerankModeError),
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
            Self::InvalidRerankBackend(error) => write!(formatter, "{error}"),
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
    let rerank = rerank_config_from_environment(overrides)?;

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
        rerank,
    })
}

fn rerank_config_from_environment(
    overrides: &RetrievalEnvOverrides,
) -> Result<RerankConfig, RetrievalRuntimeConfigError> {
    let mode = overrides
        .rerank_backend
        .as_deref()
        .map(RerankMode::parse)
        .transpose()
        .map_err(RetrievalRuntimeConfigError::InvalidRerankBackend)?
        .unwrap_or(RerankMode::Local);
    let model = match mode {
        RerankMode::Disabled => None,
        RerankMode::Local => Some(model_name_override(
            overrides.rerank_model.as_deref(),
            RELAY_KNOWLEDGE_RERANK_MODEL,
            LOCAL_RERANK_MODEL,
        )?),
        RerankMode::External => overrides
            .rerank_model
            .as_deref()
            .map(|model| model_name_override(Some(model), RELAY_KNOWLEDGE_RERANK_MODEL, ""))
            .transpose()?,
    };
    let timeout = overrides
        .rerank_timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_RERANK_TIMEOUT);

    Ok(RerankConfig {
        mode,
        model,
        timeout,
        candidate_multiplier: overrides
            .rerank_candidate_multiplier
            .unwrap_or(DEFAULT_RERANK_CANDIDATE_MULTIPLIER),
        max_candidates: overrides
            .rerank_max_candidates
            .unwrap_or(DEFAULT_RERANK_MAX_CANDIDATES),
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

fn split_semicolon(value: Option<&str>) -> Result<Vec<String>, FileIndexRuntimeConfigError> {
    value
        .map(|items| {
            items
                .split(';')
                .map(str::trim)
                .map(|item| {
                    if item.is_empty() {
                        Err(FileIndexRuntimeConfigError::EmptyListValue)
                    } else {
                        Ok(item.to_owned())
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn file_index_root_from_environment(
    scope_id: &'static str,
    root: String,
    platform: PlatformKind,
) -> Result<FileIndexRootConfig, FileIndexRuntimeConfigError> {
    if !is_absolute_file_index_root(&root, platform) {
        return Err(FileIndexRuntimeConfigError::RelativeRoot(root));
    }

    Ok(FileIndexRootConfig::new(scope_id, PathBuf::from(root)))
}

fn is_absolute_file_index_root(root: &str, platform: PlatformKind) -> bool {
    match platform {
        PlatformKind::Windows => is_absolute_windows_path(root),
        _ => PathBuf::from(root).is_absolute(),
    }
}

fn is_absolute_windows_path(root: &str) -> bool {
    let bytes = root.as_bytes();
    let drive_rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    if drive_rooted {
        return true;
    }

    if !(root.starts_with("\\\\") || root.starts_with("//")) {
        return false;
    }
    root[2..]
        .split(['\\', '/'])
        .filter(|component| !component.is_empty())
        .take(2)
        .count()
        == 2
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn agent_runtime_budget_ms(request_timeout: Duration) -> u64 {
    let budget = request_timeout.saturating_sub(Duration::from_millis(1));
    duration_millis(budget).max(1)
}

#[cfg(test)]
mod tests;
