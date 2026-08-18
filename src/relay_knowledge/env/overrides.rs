//! Typed relay-specific environment overrides.

use super::PlatformEnvironment;

/// Relay-specific path overrides read from environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathEnvOverrides {
    pub home: Option<std::path::PathBuf>,
    pub config_dir: Option<std::path::PathBuf>,
    pub data_dir: Option<std::path::PathBuf>,
    pub state_dir: Option<std::path::PathBuf>,
    pub cache_dir: Option<std::path::PathBuf>,
    pub log_dir: Option<std::path::PathBuf>,
    pub temp_dir: Option<std::path::PathBuf>,
    pub runtime_dir: Option<std::path::PathBuf>,
    pub service_dir: Option<std::path::PathBuf>,
}

/// Network settings read from relay-specific and generic environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkEnvOverrides {
    pub http_bind: Option<String>,
    pub http_request_timeout_ms: Option<u64>,
    pub http_shutdown_timeout_ms: Option<u64>,
    pub http_max_body_bytes: Option<u64>,
    pub proxy: Option<String>,
    pub no_proxy: Option<String>,
    pub ssl_verify: Option<bool>,
    pub qos_max_connections: Option<usize>,
    pub qos_max_in_flight_requests: Option<usize>,
    pub qos_max_queue_depth: Option<usize>,
}

/// Remote service settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteCliEnvOverrides {
    pub base_url: Option<String>,
}

/// Agent protocol settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentEnvOverrides {
    pub mcp_streamable_http_enabled: Option<bool>,
    pub mcp_endpoint: Option<String>,
    pub mcp_allowed_origins: Option<String>,
    pub mcp_allowed_scopes: Option<String>,
    pub mcp_allow_unspecified_scope: Option<bool>,
    pub mcp_max_limit: Option<usize>,
    pub mcp_max_context_bytes: Option<usize>,
    pub mcp_allow_remote_clients: Option<bool>,
    pub audit_sink_enabled: Option<bool>,
    pub audit_queue_depth: Option<usize>,
}

/// Retrieval backend settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetrievalEnvOverrides {
    pub semantic_backend: Option<String>,
    pub vector_backend: Option<String>,
    pub llm_provider: Option<String>,
    pub embedding_base_url: Option<String>,
    pub embedding_api_key: Option<String>,
    pub text_embedding_model: Option<String>,
    pub image_embedding_model: Option<String>,
    pub embedding_dimension: Option<usize>,
    pub embedding_batch_size: Option<usize>,
    pub embedding_timeout_ms: Option<u64>,
    pub embedding_max_concurrency: Option<usize>,
    pub rerank_backend: Option<String>,
    pub rerank_model: Option<String>,
    pub rerank_timeout_ms: Option<u64>,
    pub rerank_candidate_multiplier: Option<usize>,
    pub rerank_max_candidates: Option<usize>,
}

/// Worker and service-operator settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerEnvOverrides {
    pub embedding_endpoint: Option<String>,
    pub ocr_endpoint: Option<String>,
    pub vision_endpoint: Option<String>,
    pub extractor_endpoint: Option<String>,
    pub max_in_flight: Option<usize>,
    pub code_index_max_in_flight: Option<usize>,
    pub code_index_max_indexed_repositories: Option<usize>,
    pub silent_updates_enabled: Option<bool>,
}

/// Local file index settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileIndexEnvOverrides {
    pub enabled: Option<bool>,
    pub roots: Option<String>,
    pub excludes: Option<String>,
    pub max_depth: Option<usize>,
    pub max_file_bytes: Option<u64>,
    pub scan_interval_ms: Option<u64>,
    pub scan_timeout_ms: Option<u64>,
    pub max_files_per_root: Option<usize>,
    pub query_timeout_ms: Option<u64>,
}

/// Release update-check settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateEnvOverrides {
    pub enabled: Option<bool>,
    pub sources: Option<String>,
    pub check_interval_ms: Option<u64>,
    pub github_repo: Option<String>,
}

/// Telemetry exporter settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TelemetryEnvOverrides {
    pub otel_endpoint: Option<String>,
    pub otel_traces: Option<bool>,
    pub otel_metrics: Option<bool>,
    pub export_timeout_ms: Option<u64>,
    pub service_environment: Option<String>,
}

/// File watcher settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatcherEnvOverrides {
    pub enabled: Option<bool>,
    pub debounce_ms: Option<u64>,
    pub commit_reconcile_interval_ms: Option<u64>,
    pub max_watch_dirs: Option<usize>,
    pub hash_cache_capacity: Option<usize>,
}

/// Fully parsed process environment relevant to relay-knowledge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentConfig {
    pub platform: PlatformEnvironment,
    pub paths: PathEnvOverrides,
    pub network: NetworkEnvOverrides,
    pub remote_cli: RemoteCliEnvOverrides,
    pub agent: AgentEnvOverrides,
    pub retrieval: RetrievalEnvOverrides,
    pub workers: WorkerEnvOverrides,
    pub file_index: FileIndexEnvOverrides,
    pub updates: UpdateEnvOverrides,
    pub telemetry: TelemetryEnvOverrides,
    pub watcher: WatcherEnvOverrides,
    pub storage_topology: Option<String>,
}

/// Environment subset needed before a remote CLI command can dispatch over HTTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCliEnvironmentConfig {
    pub network: NetworkEnvOverrides,
    pub remote_cli: RemoteCliEnvOverrides,
}
