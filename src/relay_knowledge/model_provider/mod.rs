//! Model provider profiles, catalog cache, and connectivity diagnostics.
//!
//! The module owns provider configuration data and async file/network workflows.
//! It does not read environment variables directly; callers pass resolved paths,
//! network policy, and retrieval runtime metadata.

mod catalog;
mod connectivity;
mod fallback;
mod persistence;
mod profile;
mod profile_config;
mod profiles;

use std::{error::Error, fmt};

pub use catalog::{ModelCatalogModel, ModelCatalogProvider, ModelCatalogResult};
pub use connectivity::{
    ModelConnectivityDiagnostics, ModelConnectivityProbeRequest, ModelConnectivityProbeResult,
    ModelConnectivityTokenUsage, ModelDiscoveryEntry, ModelDiscoveryRequest, ModelDiscoveryResult,
};
pub use fallback::{ModelFallbackConfig, ModelFallbackPolicy, ModelFallbackStrategy};
pub use profile::{
    ModelCapabilities, ModelModalityMatrix, ModelProfileRuntimeSummary, ModelProfileSaveRequest,
    ModelProfileView, ModelProfilesResponse, ModelProviderKind, ModelRequestHeader,
};

use crate::paths::RuntimePaths;

const DEFAULT_CATALOG_SOURCE_URL: &str = "https://models.dev/api.json";

#[cfg(test)]
mod test_support;

/// Async model provider configuration service.
#[derive(Debug, Clone)]
pub struct ModelProviderConfigService {
    paths: RuntimePaths,
    catalog_source_url: String,
}

impl ModelProviderConfigService {
    pub fn new(paths: RuntimePaths) -> Self {
        Self {
            paths,
            catalog_source_url: DEFAULT_CATALOG_SOURCE_URL.to_owned(),
        }
    }
}

/// Error from model provider configuration and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelProviderError {
    InvalidInput(String),
    Io(String),
    Json(String),
    Network(String),
}

impl fmt::Display for ModelProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message)
            | Self::Io(message)
            | Self::Json(message)
            | Self::Network(message) => formatter.write_str(message),
        }
    }
}

impl Error for ModelProviderError {}

impl From<std::io::Error> for ModelProviderError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<serde_json::Error> for ModelProviderError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error.to_string())
    }
}
