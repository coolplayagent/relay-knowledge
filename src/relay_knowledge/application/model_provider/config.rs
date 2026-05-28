use crate::{
    api::ApiError,
    model_provider::{
        ModelCatalogResult, ModelConnectivityProbeRequest, ModelConnectivityProbeResult,
        ModelDiscoveryRequest, ModelDiscoveryResult, ModelFallbackConfig, ModelProfileSaveRequest,
        ModelProfilesResponse, ModelProviderError,
    },
};

use crate::application::service::RelayKnowledgeService;

impl RelayKnowledgeService {
    /// Lists redacted model provider profiles.
    pub async fn model_profiles(&self) -> Result<ModelProfilesResponse, ApiError> {
        self.model_provider_config()
            .profiles(&self.runtime.retrieval)
            .await
            .map_err(model_provider_api_error)
    }

    /// Saves a model provider profile and returns the redacted profile list.
    pub async fn save_model_profile(
        &self,
        name: &str,
        request: ModelProfileSaveRequest,
    ) -> Result<ModelProfilesResponse, ApiError> {
        self.model_provider_config()
            .save_profile(name, request, &self.runtime.retrieval)
            .await
            .map_err(model_provider_api_error)
    }

    /// Deletes a model provider profile and returns the redacted profile list.
    pub async fn delete_model_profile(
        &self,
        name: &str,
    ) -> Result<ModelProfilesResponse, ApiError> {
        self.model_provider_config()
            .delete_profile(name, &self.runtime.retrieval)
            .await
            .map_err(model_provider_api_error)
    }

    /// Returns model fallback policy configuration.
    pub async fn model_fallback_config(&self) -> Result<ModelFallbackConfig, ApiError> {
        self.model_provider_config()
            .fallback_config()
            .await
            .map_err(model_provider_api_error)
    }

    /// Saves model fallback policy configuration.
    pub async fn save_model_fallback_config(
        &self,
        config: ModelFallbackConfig,
    ) -> Result<ModelFallbackConfig, ApiError> {
        self.model_provider_config()
            .save_fallback_config(config)
            .await
            .map_err(model_provider_api_error)
    }

    /// Returns the cached or refreshed public model catalog.
    pub async fn model_catalog(&self, refresh: bool) -> Result<ModelCatalogResult, ApiError> {
        let network = self.runtime.network.current();
        self.model_provider_config()
            .catalog(&network.http, refresh)
            .await
            .map_err(model_provider_api_error)
    }

    /// Probes a configured or overridden model profile.
    pub async fn probe_model_provider(
        &self,
        request: ModelConnectivityProbeRequest,
    ) -> Result<ModelConnectivityProbeResult, ApiError> {
        let network = self.runtime.network.current();
        self.model_provider_config()
            .probe(&network.http, &self.runtime.retrieval, request)
            .await
            .map_err(model_provider_api_error)
    }

    /// Discovers models from a configured or overridden model profile.
    pub async fn discover_model_provider(
        &self,
        request: ModelDiscoveryRequest,
    ) -> Result<ModelDiscoveryResult, ApiError> {
        let network = self.runtime.network.current();
        self.model_provider_config()
            .discover(&network.http, &self.runtime.retrieval, request)
            .await
            .map_err(model_provider_api_error)
    }
}

fn model_provider_api_error(error: ModelProviderError) -> ApiError {
    match error {
        ModelProviderError::InvalidInput(message) => ApiError::invalid_argument(message),
        ModelProviderError::Io(message)
        | ModelProviderError::Json(message)
        | ModelProviderError::Network(message) => ApiError::storage_unavailable(message),
    }
}
