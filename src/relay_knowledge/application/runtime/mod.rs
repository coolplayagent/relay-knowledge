use std::{error::Error, fmt, path::PathBuf};

use crate::{
    env::{EnvError, EnvironmentConfig},
    net::{NetworkConfig, NetworkConfigError, NetworkRuntime},
    observability::{ObservabilityRuntime, TelemetryConfig},
    paths::{PathError, RuntimePaths, windows_tasklist_command},
    project::PROJECT_NAME,
    retrieval::ReadModelBackendConfig,
};

use super::update::{UpdateRuntimeConfig, UpdateRuntimeConfigError};
use retrieval::retrieval_config_from_environment;

mod agent;
mod file_index;
mod retrieval;
mod status;
mod storage;
mod worker;

pub use agent::{AgentRuntimeConfig, AgentRuntimeConfigError};
pub use file_index::{FileIndexRootConfig, FileIndexRuntimeConfig, FileIndexRuntimeConfigError};
pub use retrieval::RetrievalRuntimeConfigError;
pub(super) use status::{
    agent_protocol_status, runtime_status, runtime_status_with_model_profiles,
};
pub use storage::{StorageRuntimeConfig, StorageRuntimeConfigError};
pub use worker::{WorkerRuntimeConfig, WorkerRuntimeConfigError};

/// Resolved foundation configuration shared by all interfaces.
#[derive(Debug, Clone)]
pub struct RuntimeConfiguration {
    pub paths: RuntimePaths,
    pub process: ProcessRuntimeConfig,
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
    /// Resolves runtime configuration from a typed environment snapshot.
    pub async fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, RuntimeConfigurationError> {
        Self::from_environment_with_process(environment, ProcessRuntimeConfig::default()).await
    }

    /// Resolves configuration with process inputs captured by the bootstrap layer.
    pub async fn from_environment_with_process(
        environment: &EnvironmentConfig,
        process: ProcessRuntimeConfig,
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
            process,
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

/// Resolved process integration paths captured during runtime bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRuntimeConfig {
    pub current_executable: PathBuf,
    pub windows_tasklist_command: PathBuf,
}

impl Default for ProcessRuntimeConfig {
    fn default() -> Self {
        Self::from_bootstrap_inputs(PathBuf::from(PROJECT_NAME), None)
    }
}

impl ProcessRuntimeConfig {
    /// Builds typed process context from values captured at bootstrap.
    pub fn from_bootstrap_inputs(
        current_executable: PathBuf,
        system_root: Option<std::ffi::OsString>,
    ) -> Self {
        Self {
            current_executable,
            windows_tasklist_command: windows_tasklist_command(system_root.as_deref()),
        }
    }
}

/// Error raised while composing foundational runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeConfigurationError {
    Environment(EnvError),
    Paths(PathError),
    Network(NetworkConfigError),
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
