use std::{error::Error, fmt};

use crate::{
    env::{EnvError, EnvironmentConfig},
    net::{NetworkConfig, NetworkConfigError, NetworkRuntime, NetworkRuntimeError},
    paths::{PathError, RuntimePaths},
};

/// Resolved foundation configuration shared by all interfaces.
#[derive(Debug, Clone)]
pub struct RuntimeConfiguration {
    pub paths: RuntimePaths,
    pub network: NetworkRuntime,
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
