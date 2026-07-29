//! Process environment capture and typed configuration assembly.

use std::{env as process_env, ffi::OsString};

pub use super::overrides::{EnvironmentConfig, RemoteCliEnvironmentConfig};
use super::{
    AgentEnvOverrides, EnvError, FileIndexEnvOverrides, NetworkEnvOverrides, PathEnvOverrides,
    PlatformKind, RemoteCliEnvOverrides, RetrievalEnvOverrides, TelemetryEnvOverrides,
    UpdateEnvOverrides, WatcherEnvOverrides, WorkerEnvOverrides,
    platform::{normalize_key, platform_environment},
    value_parser::{
        EnvironmentValues, bool_var, first_bool_var, first_string_var, path_var, positive_u64_var,
        positive_usize_var, string_var,
    },
    variables::*,
};

impl RemoteCliEnvironmentConfig {
    /// Reads the current process environment for remote CLI dispatch only.
    pub fn from_process() -> Result<Self, EnvError> {
        Self::from_pairs(PlatformKind::current(), process_env::vars_os())
    }

    /// Parses a deterministic remote CLI environment snapshot.
    pub fn from_pairs<I, K, V>(platform: PlatformKind, pairs: I) -> Result<Self, EnvError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        let values = values_from_pairs(platform, pairs);

        Ok(Self {
            network: parse_network_overrides(&values)?,
            remote_cli: parse_remote_cli_overrides(&values)?,
        })
    }
}

impl EnvironmentConfig {
    /// Reads and validates the current process environment.
    pub fn from_process() -> Result<Self, EnvError> {
        Self::from_pairs(PlatformKind::current(), process_env::vars_os())
    }

    /// Parses a deterministic environment snapshot.
    pub fn from_pairs<I, K, V>(platform: PlatformKind, pairs: I) -> Result<Self, EnvError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        let values = values_from_pairs(platform, pairs);

        Ok(Self {
            platform: platform_environment(&values, platform)?,
            paths: parse_path_overrides(&values)?,
            network: parse_network_overrides(&values)?,
            remote_cli: parse_remote_cli_overrides(&values)?,
            agent: parse_agent_overrides(&values)?,
            retrieval: parse_retrieval_overrides(&values)?,
            workers: parse_worker_overrides(&values)?,
            file_index: parse_file_index_overrides(&values)?,
            updates: parse_update_overrides(&values)?,
            telemetry: parse_telemetry_overrides(&values)?,
            watcher: parse_watcher_overrides(&values)?,
            storage_topology: string_var(&values, RELAY_KNOWLEDGE_STORAGE_TOPOLOGY)?,
        })
    }
}

fn values_from_pairs<I, K, V>(platform: PlatformKind, pairs: I) -> EnvironmentValues
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
{
    pairs
        .into_iter()
        .map(|(key, value)| (normalize_key(platform, key.into()), value.into()))
        .collect()
}

fn parse_path_overrides(values: &EnvironmentValues) -> Result<PathEnvOverrides, EnvError> {
    Ok(PathEnvOverrides {
        home: path_var(values, RELAY_KNOWLEDGE_HOME)?,
        config_dir: path_var(values, RELAY_KNOWLEDGE_CONFIG_DIR)?,
        data_dir: path_var(values, RELAY_KNOWLEDGE_DATA_DIR)?,
        state_dir: path_var(values, RELAY_KNOWLEDGE_STATE_DIR)?,
        cache_dir: path_var(values, RELAY_KNOWLEDGE_CACHE_DIR)?,
        log_dir: path_var(values, RELAY_KNOWLEDGE_LOG_DIR)?,
        temp_dir: path_var(values, RELAY_KNOWLEDGE_TEMP_DIR)?,
        runtime_dir: path_var(values, RELAY_KNOWLEDGE_RUNTIME_DIR)?,
        service_dir: path_var(values, RELAY_KNOWLEDGE_SERVICE_DIR)?,
    })
}

fn parse_network_overrides(values: &EnvironmentValues) -> Result<NetworkEnvOverrides, EnvError> {
    Ok(NetworkEnvOverrides {
        http_bind: string_var(values, RELAY_KNOWLEDGE_HTTP_BIND)?,
        http_request_timeout_ms: positive_u64_var(values, RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS)?,
        http_shutdown_timeout_ms: positive_u64_var(
            values,
            RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS,
        )?,
        http_max_body_bytes: positive_u64_var(values, RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES)?,
        proxy: first_string_var(
            values,
            &[
                HTTPS_PROXY,
                HTTPS_PROXY_LOWER,
                HTTP_PROXY,
                HTTP_PROXY_LOWER,
                ALL_PROXY,
                ALL_PROXY_LOWER,
            ],
        )?,
        no_proxy: first_string_var(values, &[NO_PROXY, NO_PROXY_LOWER])?,
        ssl_verify: first_bool_var(values, &[SSL_VERIFY, SSL_VERIFY_LOWER])?,
        qos_max_connections: positive_usize_var(values, RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS)?,
        qos_max_in_flight_requests: positive_usize_var(
            values,
            RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS,
        )?,
        qos_max_queue_depth: positive_usize_var(values, RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH)?,
    })
}

fn parse_remote_cli_overrides(
    values: &EnvironmentValues,
) -> Result<RemoteCliEnvOverrides, EnvError> {
    Ok(RemoteCliEnvOverrides {
        base_url: string_var(values, RELAY_KNOWLEDGE_REMOTE_BASE_URL)?,
    })
}

fn parse_agent_overrides(values: &EnvironmentValues) -> Result<AgentEnvOverrides, EnvError> {
    Ok(AgentEnvOverrides {
        mcp_streamable_http_enabled: bool_var(values, RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED)?,
        mcp_endpoint: string_var(values, RELAY_KNOWLEDGE_MCP_ENDPOINT)?,
        mcp_allowed_origins: string_var(values, RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS)?,
        mcp_allowed_scopes: string_var(values, RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES)?,
        mcp_allow_unspecified_scope: bool_var(values, RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE)?,
        mcp_max_limit: positive_usize_var(values, RELAY_KNOWLEDGE_MCP_MAX_LIMIT)?,
        mcp_max_context_bytes: positive_usize_var(values, RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES)?,
        mcp_allow_remote_clients: bool_var(values, RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS)?,
        audit_sink_enabled: bool_var(values, RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED)?,
        audit_queue_depth: positive_usize_var(values, RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH)?,
    })
}

fn parse_retrieval_overrides(
    values: &EnvironmentValues,
) -> Result<RetrievalEnvOverrides, EnvError> {
    Ok(RetrievalEnvOverrides {
        semantic_backend: string_var(values, RELAY_KNOWLEDGE_SEMANTIC_BACKEND)?,
        vector_backend: string_var(values, RELAY_KNOWLEDGE_VECTOR_BACKEND)?,
        llm_provider: string_var(values, RELAY_KNOWLEDGE_LLM_PROVIDER)?,
        embedding_base_url: string_var(values, RELAY_KNOWLEDGE_EMBEDDING_BASE_URL)?,
        embedding_api_key: string_var(values, RELAY_KNOWLEDGE_EMBEDDING_API_KEY)?,
        text_embedding_model: string_var(values, RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL)?,
        image_embedding_model: string_var(values, RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL)?,
        embedding_dimension: positive_usize_var(values, RELAY_KNOWLEDGE_EMBEDDING_DIMENSION)?,
        embedding_batch_size: positive_usize_var(values, RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE)?,
        embedding_timeout_ms: positive_u64_var(values, RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS)?,
        embedding_max_concurrency: positive_usize_var(
            values,
            RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY,
        )?,
        rerank_backend: string_var(values, RELAY_KNOWLEDGE_RERANK_BACKEND)?,
        rerank_model: string_var(values, RELAY_KNOWLEDGE_RERANK_MODEL)?,
        rerank_timeout_ms: positive_u64_var(values, RELAY_KNOWLEDGE_RERANK_TIMEOUT_MS)?,
        rerank_candidate_multiplier: positive_usize_var(
            values,
            RELAY_KNOWLEDGE_RERANK_CANDIDATE_MULTIPLIER,
        )?,
        rerank_max_candidates: positive_usize_var(values, RELAY_KNOWLEDGE_RERANK_MAX_CANDIDATES)?,
    })
}

fn parse_worker_overrides(values: &EnvironmentValues) -> Result<WorkerEnvOverrides, EnvError> {
    Ok(WorkerEnvOverrides {
        embedding_endpoint: string_var(values, RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT)?,
        ocr_endpoint: string_var(values, RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT)?,
        vision_endpoint: string_var(values, RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT)?,
        extractor_endpoint: string_var(values, RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT)?,
        max_in_flight: positive_usize_var(values, RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT)?,
        code_index_max_in_flight: positive_usize_var(
            values,
            RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT,
        )?,
        silent_updates_enabled: bool_var(values, RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED)?,
    })
}

fn parse_file_index_overrides(
    values: &EnvironmentValues,
) -> Result<FileIndexEnvOverrides, EnvError> {
    Ok(FileIndexEnvOverrides {
        enabled: bool_var(values, RELAY_KNOWLEDGE_FILE_INDEX_ENABLED)?,
        roots: string_var(values, RELAY_KNOWLEDGE_FILE_INDEX_ROOTS)?,
        excludes: string_var(values, RELAY_KNOWLEDGE_FILE_INDEX_EXCLUDES)?,
        max_depth: positive_usize_var(values, RELAY_KNOWLEDGE_FILE_INDEX_MAX_DEPTH)?,
        max_file_bytes: positive_u64_var(values, RELAY_KNOWLEDGE_FILE_INDEX_MAX_FILE_BYTES)?,
        scan_interval_ms: positive_u64_var(values, RELAY_KNOWLEDGE_FILE_INDEX_SCAN_INTERVAL_MS)?,
        scan_timeout_ms: positive_u64_var(values, RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS)?,
        max_files_per_root: positive_usize_var(
            values,
            RELAY_KNOWLEDGE_FILE_INDEX_MAX_FILES_PER_ROOT,
        )?,
        query_timeout_ms: positive_u64_var(values, RELAY_KNOWLEDGE_FILE_QUERY_TIMEOUT_MS)?,
    })
}

fn parse_update_overrides(values: &EnvironmentValues) -> Result<UpdateEnvOverrides, EnvError> {
    let enabled = bool_var(values, RELAY_KNOWLEDGE_UPDATE_CHECK_ENABLED)?;
    let check_interval_ms = if enabled == Some(false) {
        None
    } else {
        positive_u64_var(values, RELAY_KNOWLEDGE_UPDATE_CHECK_INTERVAL_MS)?
    };

    Ok(UpdateEnvOverrides {
        enabled,
        sources: string_var(values, RELAY_KNOWLEDGE_UPDATE_SOURCES)?,
        check_interval_ms,
        github_repo: string_var(values, RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO)?,
    })
}

fn parse_telemetry_overrides(
    values: &EnvironmentValues,
) -> Result<TelemetryEnvOverrides, EnvError> {
    Ok(TelemetryEnvOverrides {
        otel_endpoint: string_var(values, RELAY_OTEL_ENDPOINT)?,
        otel_traces: bool_var(values, RELAY_OTEL_TRACES)?,
        otel_metrics: bool_var(values, RELAY_OTEL_METRICS)?,
        export_timeout_ms: positive_u64_var(values, RELAY_OTEL_EXPORT_TIMEOUT_MS)?,
        service_environment: string_var(values, RELAY_OTEL_SERVICE_ENVIRONMENT)?,
    })
}

fn parse_watcher_overrides(values: &EnvironmentValues) -> Result<WatcherEnvOverrides, EnvError> {
    Ok(WatcherEnvOverrides {
        enabled: bool_var(values, RELAY_KNOWLEDGE_WATCHER_ENABLED)?,
        debounce_ms: positive_u64_var(values, RELAY_KNOWLEDGE_WATCHER_DEBOUNCE_MS)?,
        max_watch_dirs: positive_usize_var(values, RELAY_KNOWLEDGE_WATCHER_MAX_WATCH_DIRS)?,
        hash_cache_capacity: positive_usize_var(
            values,
            RELAY_KNOWLEDGE_WATCHER_HASH_CACHE_CAPACITY,
        )?,
    })
}
