//! Environment variable boundary for runtime configuration.
//!
//! This module is the only production code that reads process environment
//! variables. It normalizes platform directory inputs and relay-specific
//! overrides into typed structures before application, path, or network code
//! consumes them.

use std::{
    collections::HashMap,
    env as process_env,
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
    path::PathBuf,
};

pub const RELAY_KNOWLEDGE_HOME: &str = "RELAY_KNOWLEDGE_HOME";
pub const RELAY_KNOWLEDGE_CONFIG_DIR: &str = "RELAY_KNOWLEDGE_CONFIG_DIR";
pub const RELAY_KNOWLEDGE_DATA_DIR: &str = "RELAY_KNOWLEDGE_DATA_DIR";
pub const RELAY_KNOWLEDGE_STATE_DIR: &str = "RELAY_KNOWLEDGE_STATE_DIR";
pub const RELAY_KNOWLEDGE_CACHE_DIR: &str = "RELAY_KNOWLEDGE_CACHE_DIR";
pub const RELAY_KNOWLEDGE_LOG_DIR: &str = "RELAY_KNOWLEDGE_LOG_DIR";
pub const RELAY_KNOWLEDGE_TEMP_DIR: &str = "RELAY_KNOWLEDGE_TEMP_DIR";
pub const RELAY_KNOWLEDGE_RUNTIME_DIR: &str = "RELAY_KNOWLEDGE_RUNTIME_DIR";
pub const RELAY_KNOWLEDGE_SERVICE_DIR: &str = "RELAY_KNOWLEDGE_SERVICE_DIR";
pub const RELAY_KNOWLEDGE_HTTP_BIND: &str = "RELAY_KNOWLEDGE_HTTP_BIND";
pub const RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS: &str = "RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS";
pub const RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS: &str =
    "RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS";
pub const RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES: &str = "RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES";
pub const RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS: &str = "RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS";
pub const RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS: &str =
    "RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS";
pub const RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH: &str = "RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH";
pub const RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED: &str =
    "RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED";
pub const RELAY_KNOWLEDGE_MCP_ENDPOINT: &str = "RELAY_KNOWLEDGE_MCP_ENDPOINT";
pub const RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS: &str = "RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS";
pub const RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES: &str = "RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES";
pub const RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE: &str =
    "RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE";
pub const RELAY_KNOWLEDGE_MCP_MAX_LIMIT: &str = "RELAY_KNOWLEDGE_MCP_MAX_LIMIT";
pub const RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES: &str = "RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES";
pub const RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH: &str = "RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH";
pub const RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS: &str =
    "RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS";
pub const RELAY_KNOWLEDGE_SEMANTIC_BACKEND: &str = "RELAY_KNOWLEDGE_SEMANTIC_BACKEND";
pub const RELAY_KNOWLEDGE_VECTOR_BACKEND: &str = "RELAY_KNOWLEDGE_VECTOR_BACKEND";
pub const RELAY_KNOWLEDGE_LLM_PROVIDER: &str = "RELAY_KNOWLEDGE_LLM_PROVIDER";
pub const RELAY_KNOWLEDGE_EMBEDDING_BASE_URL: &str = "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL";
pub const RELAY_KNOWLEDGE_EMBEDDING_API_KEY: &str = "RELAY_KNOWLEDGE_EMBEDDING_API_KEY";
pub const RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL: &str = "RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL";
pub const RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL: &str = "RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL";
pub const RELAY_KNOWLEDGE_EMBEDDING_DIMENSION: &str = "RELAY_KNOWLEDGE_EMBEDDING_DIMENSION";
pub const RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE: &str = "RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE";
pub const RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS: &str = "RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS";
pub const RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY: &str =
    "RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY";
pub const RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT: &str =
    "RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT";
pub const RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT: &str = "RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT";
pub const RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT: &str = "RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT";
pub const RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT: &str =
    "RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT";
pub const RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT: &str = "RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT";
pub const RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED: &str = "RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED";
pub const RELAY_OTEL_ENDPOINT: &str = "RELAY_OTEL_ENDPOINT";
pub const RELAY_OTEL_TRACES: &str = "RELAY_OTEL_TRACES";
pub const RELAY_OTEL_METRICS: &str = "RELAY_OTEL_METRICS";
pub const RELAY_OTEL_EXPORT_TIMEOUT_MS: &str = "RELAY_OTEL_EXPORT_TIMEOUT_MS";
pub const RELAY_OTEL_SERVICE_ENVIRONMENT: &str = "RELAY_OTEL_SERVICE_ENVIRONMENT";
pub const HTTPS_PROXY: &str = "HTTPS_PROXY";
pub const HTTPS_PROXY_LOWER: &str = "https_proxy";
pub const HTTP_PROXY: &str = "HTTP_PROXY";
pub const HTTP_PROXY_LOWER: &str = "http_proxy";
pub const ALL_PROXY: &str = "ALL_PROXY";
pub const ALL_PROXY_LOWER: &str = "all_proxy";
pub const NO_PROXY: &str = "NO_PROXY";
pub const NO_PROXY_LOWER: &str = "no_proxy";
pub const SSL_VERIFY: &str = "SSL_VERIFY";
pub const SSL_VERIFY_LOWER: &str = "ssl_verify";

const HOME: &str = "HOME";
const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";
const XDG_DATA_HOME: &str = "XDG_DATA_HOME";
const XDG_STATE_HOME: &str = "XDG_STATE_HOME";
const XDG_CACHE_HOME: &str = "XDG_CACHE_HOME";
const XDG_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";
const APPDATA: &str = "APPDATA";
const LOCALAPPDATA: &str = "LOCALAPPDATA";
const TMPDIR: &str = "TMPDIR";
const TEMP: &str = "TEMP";
const TMP: &str = "TMP";

/// Operating-system family used by path resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformKind {
    Unix,
    Macos,
    Windows,
    Other,
}

impl PlatformKind {
    /// Detects the current target platform without consulting environment state.
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(unix) {
            Self::Unix
        } else {
            Self::Other
        }
    }
}

/// Platform directory inputs captured from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformEnvironment {
    pub platform: PlatformKind,
    pub home_dir: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub temp_dir: Option<PathBuf>,
}

/// Relay-specific path overrides read from environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathEnvOverrides {
    pub home: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
    pub temp_dir: Option<PathBuf>,
    pub runtime_dir: Option<PathBuf>,
    pub service_dir: Option<PathBuf>,
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
    pub mcp_allow_index_refresh: Option<bool>,
    pub mcp_allow_remote_clients: Option<bool>,
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
}

/// Worker and service-operator settings read from relay-specific environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerEnvOverrides {
    pub embedding_endpoint: Option<String>,
    pub ocr_endpoint: Option<String>,
    pub vision_endpoint: Option<String>,
    pub extractor_endpoint: Option<String>,
    pub max_in_flight: Option<usize>,
    pub silent_updates_enabled: Option<bool>,
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

/// Fully parsed process environment relevant to relay-knowledge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentConfig {
    pub platform: PlatformEnvironment,
    pub paths: PathEnvOverrides,
    pub network: NetworkEnvOverrides,
    pub agent: AgentEnvOverrides,
    pub retrieval: RetrievalEnvOverrides,
    pub workers: WorkerEnvOverrides,
    pub telemetry: TelemetryEnvOverrides,
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
        let values = pairs
            .into_iter()
            .map(|(key, value)| (normalize_key(platform, key.into()), value.into()))
            .collect::<HashMap<_, _>>();
        let temp_variables: &[&'static str] = if platform == PlatformKind::Windows {
            &[TEMP, TMP, TMPDIR]
        } else {
            &[TMPDIR, TEMP, TMP]
        };

        Ok(Self {
            platform: PlatformEnvironment {
                platform,
                home_dir: path_var(&values, HOME)?,
                xdg_config_home: path_var(&values, XDG_CONFIG_HOME)?,
                xdg_data_home: path_var(&values, XDG_DATA_HOME)?,
                xdg_state_home: path_var(&values, XDG_STATE_HOME)?,
                xdg_cache_home: path_var(&values, XDG_CACHE_HOME)?,
                xdg_runtime_dir: path_var(&values, XDG_RUNTIME_DIR)?,
                app_data: path_var(&values, APPDATA)?,
                local_app_data: path_var(&values, LOCALAPPDATA)?,
                temp_dir: first_path_var(&values, temp_variables)?,
            },
            paths: PathEnvOverrides {
                home: path_var(&values, RELAY_KNOWLEDGE_HOME)?,
                config_dir: path_var(&values, RELAY_KNOWLEDGE_CONFIG_DIR)?,
                data_dir: path_var(&values, RELAY_KNOWLEDGE_DATA_DIR)?,
                state_dir: path_var(&values, RELAY_KNOWLEDGE_STATE_DIR)?,
                cache_dir: path_var(&values, RELAY_KNOWLEDGE_CACHE_DIR)?,
                log_dir: path_var(&values, RELAY_KNOWLEDGE_LOG_DIR)?,
                temp_dir: path_var(&values, RELAY_KNOWLEDGE_TEMP_DIR)?,
                runtime_dir: path_var(&values, RELAY_KNOWLEDGE_RUNTIME_DIR)?,
                service_dir: path_var(&values, RELAY_KNOWLEDGE_SERVICE_DIR)?,
            },
            network: NetworkEnvOverrides {
                http_bind: string_var(&values, RELAY_KNOWLEDGE_HTTP_BIND)?,
                http_request_timeout_ms: positive_u64_var(
                    &values,
                    RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS,
                )?,
                http_shutdown_timeout_ms: positive_u64_var(
                    &values,
                    RELAY_KNOWLEDGE_HTTP_SHUTDOWN_TIMEOUT_MS,
                )?,
                http_max_body_bytes: positive_u64_var(
                    &values,
                    RELAY_KNOWLEDGE_HTTP_MAX_BODY_BYTES,
                )?,
                proxy: first_string_var(
                    &values,
                    &[
                        HTTPS_PROXY,
                        HTTPS_PROXY_LOWER,
                        HTTP_PROXY,
                        HTTP_PROXY_LOWER,
                        ALL_PROXY,
                        ALL_PROXY_LOWER,
                    ],
                )?,
                no_proxy: first_string_var(&values, &[NO_PROXY, NO_PROXY_LOWER])?,
                ssl_verify: first_bool_var(&values, &[SSL_VERIFY, SSL_VERIFY_LOWER])?,
                qos_max_connections: positive_usize_var(
                    &values,
                    RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS,
                )?,
                qos_max_in_flight_requests: positive_usize_var(
                    &values,
                    RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS,
                )?,
                qos_max_queue_depth: positive_usize_var(
                    &values,
                    RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH,
                )?,
            },
            agent: AgentEnvOverrides {
                mcp_streamable_http_enabled: bool_var(
                    &values,
                    RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED,
                )?,
                mcp_endpoint: string_var(&values, RELAY_KNOWLEDGE_MCP_ENDPOINT)?,
                mcp_allowed_origins: string_var(&values, RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS)?,
                mcp_allowed_scopes: string_var(&values, RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES)?,
                mcp_allow_unspecified_scope: bool_var(
                    &values,
                    RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE,
                )?,
                mcp_max_limit: positive_usize_var(&values, RELAY_KNOWLEDGE_MCP_MAX_LIMIT)?,
                mcp_max_context_bytes: positive_usize_var(
                    &values,
                    RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES,
                )?,
                mcp_allow_index_refresh: bool_var(
                    &values,
                    RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH,
                )?,
                mcp_allow_remote_clients: bool_var(
                    &values,
                    RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS,
                )?,
            },
            retrieval: RetrievalEnvOverrides {
                semantic_backend: string_var(&values, RELAY_KNOWLEDGE_SEMANTIC_BACKEND)?,
                vector_backend: string_var(&values, RELAY_KNOWLEDGE_VECTOR_BACKEND)?,
                llm_provider: string_var(&values, RELAY_KNOWLEDGE_LLM_PROVIDER)?,
                embedding_base_url: string_var(&values, RELAY_KNOWLEDGE_EMBEDDING_BASE_URL)?,
                embedding_api_key: string_var(&values, RELAY_KNOWLEDGE_EMBEDDING_API_KEY)?,
                text_embedding_model: string_var(&values, RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL)?,
                image_embedding_model: string_var(&values, RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL)?,
                embedding_dimension: positive_usize_var(
                    &values,
                    RELAY_KNOWLEDGE_EMBEDDING_DIMENSION,
                )?,
                embedding_batch_size: positive_usize_var(
                    &values,
                    RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE,
                )?,
                embedding_timeout_ms: positive_u64_var(
                    &values,
                    RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS,
                )?,
                embedding_max_concurrency: positive_usize_var(
                    &values,
                    RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY,
                )?,
            },
            workers: WorkerEnvOverrides {
                embedding_endpoint: string_var(&values, RELAY_KNOWLEDGE_WORKER_EMBEDDING_ENDPOINT)?,
                ocr_endpoint: string_var(&values, RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT)?,
                vision_endpoint: string_var(&values, RELAY_KNOWLEDGE_WORKER_VISION_ENDPOINT)?,
                extractor_endpoint: string_var(&values, RELAY_KNOWLEDGE_WORKER_EXTRACTOR_ENDPOINT)?,
                max_in_flight: positive_usize_var(&values, RELAY_KNOWLEDGE_WORKER_MAX_IN_FLIGHT)?,
                silent_updates_enabled: bool_var(&values, RELAY_KNOWLEDGE_SILENT_UPDATES_ENABLED)?,
            },
            telemetry: TelemetryEnvOverrides {
                otel_endpoint: string_var(&values, RELAY_OTEL_ENDPOINT)?,
                otel_traces: bool_var(&values, RELAY_OTEL_TRACES)?,
                otel_metrics: bool_var(&values, RELAY_OTEL_METRICS)?,
                export_timeout_ms: positive_u64_var(&values, RELAY_OTEL_EXPORT_TIMEOUT_MS)?,
                service_environment: string_var(&values, RELAY_OTEL_SERVICE_ENVIRONMENT)?,
            },
        })
    }
}

fn normalize_key(platform: PlatformKind, key: OsString) -> OsString {
    if platform == PlatformKind::Windows {
        key.to_str()
            .map(|value| OsString::from(value.to_ascii_uppercase()))
            .unwrap_or(key)
    } else {
        key
    }
}

/// Environment parsing error with the exact variable that failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvError {
    pub variable: &'static str,
    pub kind: EnvErrorKind,
}

impl EnvError {
    fn empty(variable: &'static str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::EmptyValue,
        }
    }

    fn invalid_unicode(variable: &'static str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::InvalidUnicode,
        }
    }

    fn invalid_integer(variable: &'static str, value: &str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::InvalidInteger {
                value: value.to_owned(),
            },
        }
    }

    fn zero(variable: &'static str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::ZeroValue,
        }
    }

    fn invalid_boolean(variable: &'static str, value: &str) -> Self {
        Self {
            variable,
            kind: EnvErrorKind::InvalidBoolean {
                value: value.to_owned(),
            },
        }
    }
}

/// Error category for environment parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvErrorKind {
    EmptyValue,
    InvalidUnicode,
    InvalidInteger { value: String },
    InvalidBoolean { value: String },
    ZeroValue,
}

impl fmt::Display for EnvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            EnvErrorKind::EmptyValue => write!(formatter, "{} must not be empty", self.variable),
            EnvErrorKind::InvalidUnicode => {
                write!(formatter, "{} must be valid UTF-8", self.variable)
            }
            EnvErrorKind::InvalidInteger { value } => {
                write!(
                    formatter,
                    "{} must be a positive integer, got '{value}'",
                    self.variable
                )
            }
            EnvErrorKind::InvalidBoolean { value } => write!(
                formatter,
                "{} must be true or false, got '{value}'",
                self.variable
            ),
            EnvErrorKind::ZeroValue => {
                write!(formatter, "{} must be greater than zero", self.variable)
            }
        }
    }
}

impl Error for EnvError {}

fn path_var(
    values: &HashMap<OsString, OsString>,
    variable: &'static str,
) -> Result<Option<PathBuf>, EnvError> {
    values
        .get(OsStr::new(variable))
        .map(|value| {
            reject_empty(value, variable)?;
            Ok(PathBuf::from(value))
        })
        .transpose()
}

fn first_path_var(
    values: &HashMap<OsString, OsString>,
    variables: &[&'static str],
) -> Result<Option<PathBuf>, EnvError> {
    for variable in variables {
        if let Some(value) = path_var(values, variable)? {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

fn string_var(
    values: &HashMap<OsString, OsString>,
    variable: &'static str,
) -> Result<Option<String>, EnvError> {
    values
        .get(OsStr::new(variable))
        .map(|value| {
            reject_empty(value, variable)?;
            value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| EnvError::invalid_unicode(variable))
        })
        .transpose()
}

fn first_string_var(
    values: &HashMap<OsString, OsString>,
    variables: &[&'static str],
) -> Result<Option<String>, EnvError> {
    for variable in variables {
        if let Some(value) = string_var(values, variable)? {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

fn bool_var(
    values: &HashMap<OsString, OsString>,
    variable: &'static str,
) -> Result<Option<bool>, EnvError> {
    string_var(values, variable)?
        .map(|value| parse_bool(variable, &value))
        .transpose()
}

fn first_bool_var(
    values: &HashMap<OsString, OsString>,
    variables: &[&'static str],
) -> Result<Option<bool>, EnvError> {
    for variable in variables {
        if let Some(value) = bool_var(values, variable)? {
            return Ok(Some(value));
        }
    }

    Ok(None)
}

fn positive_u64_var(
    values: &HashMap<OsString, OsString>,
    variable: &'static str,
) -> Result<Option<u64>, EnvError> {
    string_var(values, variable)?
        .map(|value| parse_positive_u64(variable, &value))
        .transpose()
}

fn positive_usize_var(
    values: &HashMap<OsString, OsString>,
    variable: &'static str,
) -> Result<Option<usize>, EnvError> {
    string_var(values, variable)?
        .map(|value| parse_positive_usize(variable, &value))
        .transpose()
}

fn parse_positive_u64(variable: &'static str, value: &str) -> Result<u64, EnvError> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| EnvError::invalid_integer(variable, value))?;

    if parsed == 0 {
        return Err(EnvError::zero(variable));
    }

    Ok(parsed)
}

fn parse_positive_usize(variable: &'static str, value: &str) -> Result<usize, EnvError> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| EnvError::invalid_integer(variable, value))?;

    if parsed == 0 {
        return Err(EnvError::zero(variable));
    }

    Ok(parsed)
}

fn parse_bool(variable: &'static str, value: &str) -> Result<bool, EnvError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(EnvError::invalid_boolean(variable, value)),
    }
}

fn reject_empty(value: &OsString, variable: &'static str) -> Result<(), EnvError> {
    if value.is_empty() {
        return Err(EnvError::empty(variable));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_platform_and_relay_overrides() {
        let config = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                (HOME, "/home/alice"),
                (TMPDIR, "/var/tmp"),
                (RELAY_KNOWLEDGE_HOME, "/opt/relay-runtime"),
                (RELAY_KNOWLEDGE_HTTP_BIND, "127.0.0.1:9000"),
                (HTTPS_PROXY, "https://proxy.internal:8443"),
                (NO_PROXY, "localhost,.internal"),
                (SSL_VERIFY, "false"),
                (RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS, "512"),
                (RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED, "true"),
                (RELAY_KNOWLEDGE_MCP_ENDPOINT, "/mcp"),
                (RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS, "http://localhost:8791"),
                (RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES, "docs,src"),
                (RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE, "true"),
                (RELAY_KNOWLEDGE_MCP_MAX_LIMIT, "5"),
                (RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES, "8192"),
                (RELAY_KNOWLEDGE_MCP_ALLOW_INDEX_REFRESH, "true"),
                (RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS, "false"),
                (RELAY_KNOWLEDGE_SEMANTIC_BACKEND, "external"),
                (RELAY_KNOWLEDGE_VECTOR_BACKEND, "external"),
                (RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL, "text-embed-3-small"),
                (RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL, "clip-vit-b32"),
                (RELAY_KNOWLEDGE_EMBEDDING_DIMENSION, "1536"),
            ],
        )
        .expect("environment should parse");

        assert_eq!(config.platform.platform, PlatformKind::Unix);
        assert_eq!(config.platform.home_dir, Some(PathBuf::from("/home/alice")));
        assert_eq!(config.platform.temp_dir, Some(PathBuf::from("/var/tmp")));
        assert_eq!(config.paths.home, Some(PathBuf::from("/opt/relay-runtime")));
        assert_eq!(config.network.http_bind, Some("127.0.0.1:9000".to_owned()));
        assert_eq!(
            config.network.proxy,
            Some("https://proxy.internal:8443".to_owned())
        );
        assert_eq!(
            config.network.no_proxy,
            Some("localhost,.internal".to_owned())
        );
        assert_eq!(config.network.ssl_verify, Some(false));
        assert_eq!(config.network.qos_max_connections, Some(512));
        assert_eq!(config.agent.mcp_streamable_http_enabled, Some(true));
        assert_eq!(config.agent.mcp_endpoint, Some("/mcp".to_owned()));
        assert_eq!(
            config.agent.mcp_allowed_origins,
            Some("http://localhost:8791".to_owned())
        );
        assert_eq!(config.agent.mcp_allowed_scopes, Some("docs,src".to_owned()));
        assert_eq!(config.agent.mcp_allow_unspecified_scope, Some(true));
        assert_eq!(config.agent.mcp_max_limit, Some(5));
        assert_eq!(config.agent.mcp_max_context_bytes, Some(8192));
        assert_eq!(config.agent.mcp_allow_index_refresh, Some(true));
        assert_eq!(config.agent.mcp_allow_remote_clients, Some(false));
        assert_eq!(
            config.retrieval.semantic_backend,
            Some("external".to_owned())
        );
        assert_eq!(config.retrieval.vector_backend, Some("external".to_owned()));
        assert_eq!(
            config.retrieval.text_embedding_model,
            Some("text-embed-3-small".to_owned())
        );
        assert_eq!(
            config.retrieval.image_embedding_model,
            Some("clip-vit-b32".to_owned())
        );
        assert_eq!(config.retrieval.embedding_dimension, Some(1536));
    }

    #[test]
    fn rejects_empty_path_values() {
        let error = EnvironmentConfig::from_pairs(PlatformKind::Unix, [(RELAY_KNOWLEDGE_HOME, "")])
            .expect_err("empty path should fail");

        assert_eq!(error.variable, RELAY_KNOWLEDGE_HOME);
        assert_eq!(error.kind, EnvErrorKind::EmptyValue);
    }

    #[test]
    fn rejects_invalid_numeric_values() {
        let error = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [(RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS, "many")],
        )
        .expect_err("invalid numeric value should fail");

        assert_eq!(error.variable, RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS);
        assert_eq!(
            error.kind,
            EnvErrorKind::InvalidInteger {
                value: "many".to_owned()
            }
        );
    }

    #[test]
    fn rejects_zero_numeric_values() {
        let error = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [(RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS, "0")],
        )
        .expect_err("zero timeout should fail");

        assert_eq!(error.variable, RELAY_KNOWLEDGE_HTTP_REQUEST_TIMEOUT_MS);
        assert_eq!(error.kind, EnvErrorKind::ZeroValue);
    }

    #[test]
    fn https_proxy_takes_precedence_over_http_proxy() {
        let config = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                (HTTP_PROXY, "http://http-proxy:8080"),
                (HTTPS_PROXY, "http://https-proxy:8080"),
                (NO_PROXY, "localhost"),
            ],
        )
        .expect("environment should parse");

        assert_eq!(
            config.network.proxy,
            Some("http://https-proxy:8080".to_owned())
        );
        assert_eq!(config.network.no_proxy, Some("localhost".to_owned()));
    }

    #[test]
    fn rejects_invalid_boolean_values() {
        let error = EnvironmentConfig::from_pairs(PlatformKind::Unix, [(SSL_VERIFY, "sometimes")])
            .expect_err("invalid boolean should fail");

        assert_eq!(error.variable, SSL_VERIFY);
        assert_eq!(
            error.kind,
            EnvErrorKind::InvalidBoolean {
                value: "sometimes".to_owned()
            }
        );
    }

    #[test]
    fn windows_environment_names_are_case_insensitive() {
        let config = EnvironmentConfig::from_pairs(
            PlatformKind::Windows,
            [
                ("home", "/home/alice"),
                ("appdata", "/roaming"),
                ("localappdata", "/local"),
                ("relay_knowledge_http_bind", "localhost:8791"),
                ("ssl_verify", "off"),
            ],
        )
        .expect("environment should parse");

        assert_eq!(config.platform.home_dir, Some(PathBuf::from("/home/alice")));
        assert_eq!(config.platform.app_data, Some(PathBuf::from("/roaming")));
        assert_eq!(
            config.platform.local_app_data,
            Some(PathBuf::from("/local"))
        );
        assert_eq!(config.network.http_bind, Some("localhost:8791".to_owned()));
        assert_eq!(config.network.ssl_verify, Some(false));
    }

    #[test]
    fn windows_temp_prefers_temp_tmp_before_tmpdir() {
        let config = EnvironmentConfig::from_pairs(
            PlatformKind::Windows,
            [
                (TMPDIR, "/posix/tmp"),
                (TEMP, "/windows/temp"),
                (TMP, "/windows/tmp"),
            ],
        )
        .expect("environment should parse");

        assert_eq!(
            config.platform.temp_dir,
            Some(PathBuf::from("/windows/temp"))
        );
    }
}
