//! Outermost service assembly for concrete runtime adapters.

use std::sync::Arc;

use crate::{
    adapters::{NetworkEmbeddingProvider, NetworkWorkerOutbound, SqliteKnowledgeStoreFactory},
    application::{RelayKnowledgeService, RuntimeConfiguration, RuntimeConfigurationError},
    env::EnvironmentConfig,
    ports::{embedding::EmbeddingProvider, worker_outbound::WorkerOutboundPort},
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
        RuntimeConfiguration::from_process_environment()
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
