//! Network configuration and policy boundary.
//!
//! All network-facing code must enter through this module or its children.
//! The current foundation layer defines event-driven HTTP configuration and
//! QoS admission policy without opening sockets or starting unmanaged loops.

use std::{
    error::Error,
    fmt,
    sync::{Arc, RwLock},
};

use crate::env::{EnvError, EnvironmentConfig, NetworkEnvOverrides};

pub mod http;
pub mod qos;

/// Resolved network policy shared by future HTTP clients and servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkConfig {
    pub http: http::HttpConfig,
    pub qos: qos::QosPolicy,
}

impl NetworkConfig {
    /// Resolves environment overrides into validated network configuration.
    pub fn from_overrides(overrides: &NetworkEnvOverrides) -> Result<Self, NetworkConfigError> {
        Ok(Self {
            http: http::HttpConfig::from_overrides(overrides).map_err(NetworkConfigError::Http)?,
            qos: qos::QosPolicy::from_overrides(overrides).map_err(NetworkConfigError::Qos)?,
        })
    }
}

/// Refreshable network configuration shared by network adapters.
#[derive(Debug, Clone)]
pub struct NetworkRuntime {
    inner: Arc<RwLock<NetworkConfig>>,
}

impl NetworkRuntime {
    /// Creates a refreshable handle from validated network configuration.
    pub fn from_config(config: NetworkConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(config)),
        }
    }

    /// Creates a refreshable handle from environment overrides.
    pub fn from_overrides(overrides: &NetworkEnvOverrides) -> Result<Self, NetworkConfigError> {
        NetworkConfig::from_overrides(overrides).map(Self::from_config)
    }

    /// Returns the latest validated network configuration.
    pub fn current(&self) -> NetworkConfig {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Replaces the active network configuration after validating overrides.
    pub fn refresh_from_overrides(
        &self,
        overrides: &NetworkEnvOverrides,
    ) -> Result<NetworkConfig, NetworkConfigError> {
        let config = NetworkConfig::from_overrides(overrides)?;

        *self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = config.clone();

        Ok(config)
    }

    /// Replaces the active network configuration from a typed environment snapshot.
    pub fn refresh_from_environment(
        &self,
        environment: &EnvironmentConfig,
    ) -> Result<NetworkConfig, NetworkConfigError> {
        self.refresh_from_overrides(&environment.network)
    }

    /// Re-reads the current process environment and applies network changes.
    pub fn refresh_from_process_environment(&self) -> Result<NetworkConfig, NetworkRuntimeError> {
        let environment =
            EnvironmentConfig::from_process().map_err(NetworkRuntimeError::Environment)?;

        self.refresh_from_environment(&environment)
            .map_err(NetworkRuntimeError::Config)
    }
}

/// Network configuration error grouped by owning submodule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkConfigError {
    Http(http::HttpConfigError),
    Qos(qos::QosPolicyError),
}

impl fmt::Display for NetworkConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(formatter, "invalid HTTP configuration: {error}"),
            Self::Qos(error) => write!(formatter, "invalid QoS policy: {error}"),
        }
    }
}

impl Error for NetworkConfigError {}

/// Error raised while refreshing network config from live environment state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkRuntimeError {
    Environment(EnvError),
    Config(NetworkConfigError),
}

impl fmt::Display for NetworkRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment(error) => write!(formatter, "{error}"),
            Self::Config(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for NetworkRuntimeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::PlatformKind;

    #[test]
    fn resolves_default_network_configuration() {
        let config = NetworkConfig::from_overrides(&NetworkEnvOverrides::default())
            .expect("defaults should resolve");

        assert_eq!(config.http.bind_address.to_string(), "127.0.0.1:8791");
        assert!(!config.http.proxy.is_proxy_configured());
        assert!(config.http.proxy.ssl_verify);
        assert_eq!(config.qos.max_connections, 1024);
        assert_eq!(config.qos.max_in_flight_requests, 256);
        assert_eq!(config.qos.max_queue_depth, 512);
    }

    #[test]
    fn refreshes_runtime_network_config_from_environment_snapshot() {
        let runtime = NetworkRuntime::from_overrides(&NetworkEnvOverrides::default())
            .expect("runtime should build");
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HTTP_PROXY", "http://relay-proxy:8080"),
                ("NO_PROXY", "localhost"),
                ("SSL_VERIFY", "false"),
                ("RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS", "8"),
            ],
        )
        .expect("environment should parse");

        runtime
            .refresh_from_environment(&environment)
            .expect("network refresh should succeed");
        let config = runtime.current();

        assert_eq!(
            config.http.proxy.proxy,
            Some("http://relay-proxy:8080".to_owned())
        );
        assert_eq!(config.http.proxy.no_proxy_rules, ["localhost"]);
        assert!(!config.http.proxy.ssl_verify);
        assert_eq!(config.qos.max_connections, 8);
    }
}
