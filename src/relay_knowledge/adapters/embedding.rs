//! Remote embedding transport assembled from network capabilities.

use crate::{
    net::{NetworkRuntime, http},
    ports::embedding::{
        EmbeddingFuture, EmbeddingProvider, EmbeddingProviderError, EmbeddingRequest,
        EmbeddingVector, ProviderRetryClass,
    },
    retrieval::{RemoteEmbeddingConfig, provider::embedding_provider_with_qos},
};

/// Lazy provider adapter that builds its HTTP client at request time.
pub struct NetworkEmbeddingProvider {
    config: RemoteEmbeddingConfig,
    network: NetworkRuntime,
}

impl NetworkEmbeddingProvider {
    pub fn new(config: RemoteEmbeddingConfig, network: NetworkRuntime) -> Self {
        Self { config, network }
    }
}

impl EmbeddingProvider for NetworkEmbeddingProvider {
    fn embed(&self, request: EmbeddingRequest) -> EmbeddingFuture<'_, Vec<EmbeddingVector>> {
        Box::pin(async move {
            let network = self.network.current();
            let client = http::outbound_json_client(&network.http).map_err(|error| {
                EmbeddingProviderError {
                    retry: ProviderRetryClass::Permanent,
                    status_code: None,
                    code: "client_build_failed".to_owned(),
                    message: error.to_string(),
                }
            })?;
            embedding_provider_with_qos(
                self.config.clone(),
                client,
                self.network.qos_runtime(),
                network.qos,
            )
            .embed(request)
            .await
        })
    }
}
