//! Application services that orchestrate domain behavior behind stable API types.

use std::{error::Error, fmt, path::Path, time::Duration};

use crate::{
    api::{ApiMetadata, ProjectStatusResponse, RequestContext, RuntimeStatus},
    domain::GraphVersion,
    env::{EnvError, EnvironmentConfig},
    net::{NetworkConfig, NetworkConfigError, NetworkRuntime, NetworkRuntimeError},
    paths::{PathError, RuntimePaths},
    project_name,
};

/// Resolved foundation configuration shared by all interfaces.
#[derive(Debug, Clone)]
pub struct RuntimeConfiguration {
    pub paths: RuntimePaths,
    pub network: NetworkRuntime,
}

impl RuntimeConfiguration {
    /// Resolves runtime configuration from the current process environment.
    pub fn from_process_environment() -> Result<Self, RuntimeConfigurationError> {
        let environment =
            EnvironmentConfig::from_process().map_err(RuntimeConfigurationError::Environment)?;

        Self::from_environment(&environment)
    }

    /// Resolves runtime configuration from a typed environment snapshot.
    pub fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, RuntimeConfigurationError> {
        Ok(Self {
            paths: RuntimePaths::resolve(&environment.platform, &environment.paths)
                .map_err(RuntimeConfigurationError::Paths)?,
            network: NetworkRuntime::from_config(
                NetworkConfig::from_overrides(&environment.network)
                    .map_err(RuntimeConfigurationError::Network)?,
            ),
        })
    }
}

/// Error raised while composing foundational runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeConfigurationError {
    Environment(EnvError),
    Paths(PathError),
    Network(NetworkConfigError),
    NetworkRuntime(NetworkRuntimeError),
}

impl fmt::Display for RuntimeConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment(error) => write!(formatter, "{error}"),
            Self::Paths(error) => write!(formatter, "{error}"),
            Self::Network(error) => write!(formatter, "{error}"),
            Self::NetworkRuntime(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for RuntimeConfigurationError {}

/// Shared application service used by CLI, Web, and future API adapters.
#[derive(Debug, Clone)]
pub struct RelayKnowledgeService {
    runtime: RuntimeConfiguration,
}

impl RelayKnowledgeService {
    /// Creates a service from already validated foundational configuration.
    pub fn new(runtime: RuntimeConfiguration) -> Self {
        Self { runtime }
    }

    /// Creates a service by reading the current process environment once.
    pub fn from_process_environment() -> Result<Self, RuntimeConfigurationError> {
        RuntimeConfiguration::from_process_environment().map(Self::new)
    }

    /// Creates a service from a deterministic environment snapshot.
    pub fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, RuntimeConfigurationError> {
        RuntimeConfiguration::from_environment(environment).map(Self::new)
    }

    /// Applies network-related settings from a typed environment snapshot.
    pub fn refresh_network_from_environment(
        &self,
        environment: &EnvironmentConfig,
    ) -> Result<(), RuntimeConfigurationError> {
        self.runtime
            .network
            .refresh_from_environment(environment)
            .map(|_| ())
            .map_err(RuntimeConfigurationError::Network)
    }

    /// Re-reads process environment variables and applies network changes.
    pub fn refresh_network_from_process_environment(
        &self,
    ) -> Result<(), RuntimeConfigurationError> {
        self.runtime
            .network
            .refresh_from_process_environment()
            .map(|_| ())
            .map_err(RuntimeConfigurationError::NetworkRuntime)
    }

    /// Returns the current project status through the unified API contract.
    pub fn project_status(&self, context: RequestContext) -> ProjectStatusResponse {
        ProjectStatusResponse {
            project_name: project_name().to_owned(),
            metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
            runtime: runtime_status(&self.runtime),
        }
    }
}

fn runtime_status(runtime: &RuntimeConfiguration) -> RuntimeStatus {
    let network = runtime.network.current();

    RuntimeStatus {
        config_dir: path_string(&runtime.paths.config_dir),
        data_dir: path_string(&runtime.paths.data_dir),
        state_dir: path_string(&runtime.paths.state_dir),
        cache_dir: path_string(&runtime.paths.cache_dir),
        log_dir: path_string(&runtime.paths.log_dir),
        temp_dir: path_string(&runtime.paths.temp_dir),
        runtime_dir: path_string(&runtime.paths.runtime_dir),
        service_dir: path_string(&runtime.paths.service_dir),
        http_bind: network.http.bind_address.to_string(),
        http_request_timeout_ms: duration_millis(network.http.request_timeout),
        http_graceful_shutdown_timeout_ms: duration_millis(network.http.graceful_shutdown_timeout),
        http_max_request_body_bytes: network.http.max_request_body_bytes,
        http_proxy_configured: network.http.proxy.is_proxy_configured(),
        http_no_proxy_rules: network.http.proxy.no_proxy_rules.len(),
        http_ssl_verify: network.http.proxy.ssl_verify,
        qos_max_connections: network.qos.max_connections,
        qos_max_in_flight_requests: network.qos.max_in_flight_requests,
        qos_max_queue_depth: network.qos.max_queue_depth,
    }
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::InterfaceKind, env::PlatformKind};

    #[test]
    fn status_includes_foundational_runtime_configuration() {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("TMPDIR", "/tmp"),
                ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
                ("RELAY_KNOWLEDGE_HTTP_BIND", "127.0.0.1:9000"),
                ("HTTPS_PROXY", "https://proxy.internal:8443"),
                ("NO_PROXY", "localhost,.internal"),
                ("SSL_VERIFY", "false"),
                ("RELAY_KNOWLEDGE_QOS_MAX_QUEUE_DEPTH", "42"),
            ],
        )
        .expect("environment should parse");
        let service =
            RelayKnowledgeService::from_environment(&environment).expect("service should compose");
        let context = RequestContext::with_ids(InterfaceKind::Cli, "req", "trace");

        let response = service.project_status(context);

        assert_eq!(response.runtime.config_dir, "/srv/relay/config");
        assert_eq!(response.runtime.data_dir, "/srv/relay/data");
        assert_eq!(response.runtime.http_bind, "127.0.0.1:9000");
        assert!(response.runtime.http_proxy_configured);
        assert_eq!(response.runtime.http_no_proxy_rules, 2);
        assert!(!response.runtime.http_ssl_verify);
        assert_eq!(response.runtime.qos_max_queue_depth, 42);
    }

    #[test]
    fn status_reflects_refreshed_network_environment() {
        let initial_environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/home/alice"),
                ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ],
        )
        .expect("environment should parse");
        let service = RelayKnowledgeService::from_environment(&initial_environment)
            .expect("service should compose");

        let refreshed_environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HTTP_PROXY", "http://proxy.internal:8080"),
                ("SSL_VERIFY", "false"),
                ("RELAY_KNOWLEDGE_QOS_MAX_IN_FLIGHT_REQUESTS", "4"),
            ],
        )
        .expect("environment should parse");

        service
            .refresh_network_from_environment(&refreshed_environment)
            .expect("network refresh should succeed");
        let response =
            service.project_status(RequestContext::with_ids(InterfaceKind::Cli, "req", "trace"));

        assert!(response.runtime.http_proxy_configured);
        assert!(!response.runtime.http_ssl_verify);
        assert_eq!(response.runtime.qos_max_in_flight_requests, 4);
    }
}
