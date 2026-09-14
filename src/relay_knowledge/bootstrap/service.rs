//! Outermost service assembly for concrete runtime adapters.

use std::sync::Arc;

use crate::{
    adapters::{NetworkEmbeddingProvider, NetworkWorkerOutbound, SqliteKnowledgeStoreFactory},
    application::{
        ProcessRuntimeConfig, RelayKnowledgeService, RuntimeConfiguration,
        RuntimeConfigurationError,
    },
    env::{EnvironmentConfig, windows_system_root_from_process},
    ports::{embedding::EmbeddingProvider, worker_outbound::WorkerOutboundPort},
    project::PROJECT_NAME,
    storage::{KnowledgeStore, KnowledgeStoreFactory},
};

impl RelayKnowledgeService {
    /// Creates a service from validated configuration and outer storage adapters.
    pub fn new(runtime: RuntimeConfiguration) -> Self {
        let storage: Arc<dyn KnowledgeStoreFactory> = Arc::new(SqliteKnowledgeStoreFactory::new(
            runtime.paths.clone(),
            runtime.storage.topology,
        ));
        let adapters = network_adapters(&runtime);
        Self::with_runtime_adapters(runtime, storage, adapters.embedding, adapters.worker)
    }

    /// Creates a service backed by an explicit store for deterministic tests.
    pub fn with_store(runtime: RuntimeConfiguration, store: Arc<dyn KnowledgeStore>) -> Self {
        let adapters = network_adapters(&runtime);
        Self::with_store_and_runtime_adapters(runtime, store, adapters.embedding, adapters.worker)
    }

    /// Creates a service by reading the current process environment once.
    pub async fn from_process_environment() -> Result<Self, RuntimeConfigurationError> {
        runtime_configuration_from_process_environment()
            .await
            .map(Self::new)
    }

    /// Creates a service from a deterministic environment snapshot.
    pub async fn from_environment(
        environment: &EnvironmentConfig,
    ) -> Result<Self, RuntimeConfigurationError> {
        RuntimeConfiguration::from_environment(environment)
            .await
            .map(Self::new)
    }

    /// Creates a service from deterministic environment and process snapshots.
    pub async fn from_environment_with_process(
        environment: &EnvironmentConfig,
        process: ProcessRuntimeConfig,
    ) -> Result<Self, RuntimeConfigurationError> {
        RuntimeConfiguration::from_environment_with_process(environment, process)
            .await
            .map(Self::new)
    }
}

/// Captures live process inputs at the composition root before application
/// configuration is resolved from typed snapshots.
pub(crate) async fn runtime_configuration_from_process_environment()
-> Result<RuntimeConfiguration, RuntimeConfigurationError> {
    let environment =
        EnvironmentConfig::from_process().map_err(RuntimeConfigurationError::Environment)?;
    let current_executable =
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from(PROJECT_NAME));
    let process = ProcessRuntimeConfig::from_bootstrap_inputs(
        current_executable,
        windows_system_root_from_process(),
    );

    RuntimeConfiguration::from_environment_with_process(&environment, process).await
}

struct RuntimeNetworkAdapters {
    embedding: Option<Arc<dyn EmbeddingProvider>>,
    worker: Option<Arc<dyn WorkerOutboundPort>>,
}

fn network_adapters(runtime: &RuntimeConfiguration) -> RuntimeNetworkAdapters {
    let embedding = runtime.retrieval.remote_embedding.clone().map(|config| {
        Arc::new(NetworkEmbeddingProvider::new(
            config,
            runtime.network.clone(),
        )) as Arc<dyn EmbeddingProvider>
    });
    let worker = Some(
        Arc::new(NetworkWorkerOutbound::new(runtime.network.clone()))
            as Arc<dyn WorkerOutboundPort>,
    );
    RuntimeNetworkAdapters { embedding, worker }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
