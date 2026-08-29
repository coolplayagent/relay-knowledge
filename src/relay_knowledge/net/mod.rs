//! Network configuration and policy boundary.
//!
//! All network-facing code must enter through this module or its children.
//! This boundary owns event-driven HTTP client and server construction,
//! listener/socket setup, proxy and TLS policy, request and shutdown timeouts,
//! and QoS admission. Higher layers supply routers, payloads, and domain
//! handlers; they should not open sockets, build HTTP clients, or run protocol
//! loops outside `net`.

use std::{
    error::Error,
    fmt,
    sync::{Arc, RwLock},
};

use crate::env::{EnvironmentConfig, NetworkEnvOverrides};

pub mod http;
pub mod qos;

/// Resolved network policy shared by HTTP clients and servers.
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
    qos: qos::QosRuntime,
}

impl NetworkRuntime {
    /// Creates a refreshable handle from validated network configuration.
    pub fn from_config(config: NetworkConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(config)),
            qos: qos::QosRuntime::default(),
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

    /// Returns the shared QoS runtime counters for network adapters.
    pub fn qos_runtime(&self) -> qos::QosRuntime {
        self.qos.clone()
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
