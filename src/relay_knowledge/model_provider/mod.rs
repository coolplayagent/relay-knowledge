//! Model provider profiles, catalog cache, and connectivity diagnostics.
//!
//! The module owns provider configuration data and async file/network workflows.
//! It does not read environment variables directly; callers pass resolved paths,
//! network policy, and retrieval runtime metadata.

mod helpers;

use std::{collections::BTreeMap, error::Error, fmt, time::Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;

use helpers::*;

use crate::{
    net::http::HttpConfig,
    paths::RuntimePaths,
    retrieval::{EmbeddingProviderKind, ReadModelBackendConfig},
};

const DEFAULT_PROFILE_NAME: &str = "default";
const DEFAULT_CATALOG_SOURCE_URL: &str = "https://models.dev/api.json";
const DEFAULT_CONNECT_TIMEOUT_SECONDS: f64 = 30.0;
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_CODEAGENT_BASE_URL: &str = "https://codeagentcli.rnd.huawei.com/codeAgentPro";
const DEFAULT_MAAS_BASE_URL: &str =
    "http://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2/";

/// Model provider family accepted by profile configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProviderKind {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    Anthropic,
    Bigmodel,
    Minimax,
    Maas,
    Codeagent,
    Echo,
}

impl ModelProviderKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::Anthropic => "anthropic",
            Self::Bigmodel => "bigmodel",
            Self::Minimax => "minimax",
            Self::Maas => "maas",
            Self::Codeagent => "codeagent",
            Self::Echo => "echo",
        }
    }

    const fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some(DEFAULT_ANTHROPIC_BASE_URL),
            Self::Codeagent => Some(DEFAULT_CODEAGENT_BASE_URL),
            Self::Maas => Some(DEFAULT_MAAS_BASE_URL),
            Self::Echo => Some("http://127.0.0.1/echo"),
            Self::OpenAiCompatible | Self::Bigmodel | Self::Minimax => None,
        }
    }
}

/// Secret-bearing request header configured for a model profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRequestHeader {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub configured: bool,
}

impl ModelRequestHeader {
    fn normalized(mut self) -> Result<Self, ModelProviderError> {
        self.name = non_empty_string(self.name, "header name")?;
        self.value = self
            .value
            .and_then(|value| non_empty_string(value, "header value").ok());
        self.configured = self.configured || self.value.is_some();
        Ok(self)
    }

    fn redacted(&self) -> Self {
        Self {
            name: self.name.clone(),
            value: (!self.secret).then(|| self.value.clone()).flatten(),
            secret: self.secret,
            configured: self.configured || self.value.is_some(),
        }
    }
}

/// Optional model capability matrix surfaced in Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    #[serde(default)]
    pub input: ModelModalityMatrix,
    #[serde(default)]
    pub output: ModelModalityMatrix,
}

/// Capability flags per modality.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelModalityMatrix {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf: Option<bool>,
}

/// User-editable profile payload used by the Web API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfileSaveRequest {
    pub provider: ModelProviderKind,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
    #[serde(default)]
    pub headers: Vec<ModelRequestHeader>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_verify: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_top_p")]
    pub top_p: f64,
    #[serde(default = "default_connect_timeout_seconds")]
    pub connect_timeout_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_policy_id: Option<String>,
    #[serde(default)]
    pub fallback_priority: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_model_name: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

/// Redacted profile returned by diagnostics and Web Settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfileView {
    pub name: String,
    pub provider: ModelProviderKind,
    pub model: String,
    pub base_url: String,
    pub api_key_configured: bool,
    pub headers: Vec<ModelRequestHeader>,
    pub ssl_verify: Option<bool>,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
    pub temperature: f64,
    pub top_p: f64,
    pub connect_timeout_seconds: f64,
    pub capabilities: ModelCapabilities,
    pub fallback_policy_id: Option<String>,
    pub fallback_priority: u32,
    pub catalog_provider_id: Option<String>,
    pub catalog_provider_name: Option<String>,
    pub catalog_model_name: Option<String>,
    pub is_default: bool,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredModelProfile {
    provider: ModelProviderKind,
    model: String,
    base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    #[serde(default)]
    headers: Vec<ModelRequestHeader>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ssl_verify: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    temperature: f64,
    top_p: f64,
    connect_timeout_seconds: f64,
    #[serde(default)]
    capabilities: ModelCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback_policy_id: Option<String>,
    fallback_priority: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_model_name: Option<String>,
    #[serde(default)]
    is_default: bool,
    source: String,
}

impl StoredModelProfile {
    fn from_save_request(
        request: ModelProfileSaveRequest,
        existing: Option<&Self>,
    ) -> Result<Self, ModelProviderError> {
        validate_sampling(
            request.temperature,
            request.top_p,
            request.connect_timeout_seconds,
        )?;
        let provider = request.provider;
        let model = non_empty_string(request.model, "model")?;
        let base_url = normalized_base_url(provider, request.base_url)?;
        let api_key = if request.clear_api_key {
            None
        } else {
            match request.api_key {
                Some(value) => non_empty_string(value, "api_key").ok(),
                None => existing.and_then(|profile| profile.api_key.clone()),
            }
        };
        let headers = if request.headers.is_empty() {
            existing
                .map(|profile| profile.headers.clone())
                .unwrap_or_default()
        } else {
            validate_headers(
                request.headers,
                existing.map(|profile| profile.headers.as_slice()),
            )?
        };
        if !provider_allows_missing_auth(provider)
            && api_key.is_none()
            && !headers.iter().any(|header| header.configured)
        {
            return Err(ModelProviderError::InvalidInput(
                "model profile requires api_key or at least one configured header".to_owned(),
            ));
        }

        Ok(Self {
            provider,
            model,
            base_url,
            api_key,
            headers,
            ssl_verify: request.ssl_verify,
            context_window: request.context_window,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            top_p: request.top_p,
            connect_timeout_seconds: request.connect_timeout_seconds,
            capabilities: request.capabilities.unwrap_or_default(),
            fallback_policy_id: request.fallback_policy_id.and_then(normalize_optional),
            fallback_priority: request.fallback_priority,
            catalog_provider_id: request.catalog_provider_id.and_then(normalize_optional),
            catalog_provider_name: request.catalog_provider_name.and_then(normalize_optional),
            catalog_model_name: request.catalog_model_name.and_then(normalize_optional),
            is_default: request.is_default,
            source: "config".to_owned(),
        })
    }

    fn from_runtime(retrieval: &ReadModelBackendConfig) -> Option<Self> {
        let remote = retrieval.remote_embedding.as_ref()?;
        Some(Self {
            provider: match remote.provider {
                EmbeddingProviderKind::OpenAiCompatible => ModelProviderKind::OpenAiCompatible,
                EmbeddingProviderKind::Echo => ModelProviderKind::Echo,
            },
            model: retrieval.vector_model.name.clone(),
            base_url: remote.base_url.clone(),
            api_key: Some(remote.api_key.clone()),
            headers: Vec::new(),
            ssl_verify: None,
            context_window: None,
            max_tokens: None,
            temperature: default_temperature(),
            top_p: default_top_p(),
            connect_timeout_seconds: default_connect_timeout_seconds(),
            capabilities: ModelCapabilities {
                input: ModelModalityMatrix {
                    text: Some(true),
                    image: None,
                    audio: None,
                    video: None,
                    pdf: None,
                },
                output: ModelModalityMatrix {
                    text: Some(true),
                    image: None,
                    audio: None,
                    video: None,
                    pdf: None,
                },
            },
            fallback_policy_id: None,
            fallback_priority: 0,
            catalog_provider_id: None,
            catalog_provider_name: None,
            catalog_model_name: None,
            is_default: true,
            source: "environment".to_owned(),
        })
    }

    fn to_view(&self, name: &str, is_default: bool) -> ModelProfileView {
        ModelProfileView {
            name: name.to_owned(),
            provider: self.provider,
            model: self.model.clone(),
            base_url: redacted_url(&self.base_url),
            api_key_configured: self.api_key.is_some(),
            headers: self
                .headers
                .iter()
                .map(ModelRequestHeader::redacted)
                .collect(),
            ssl_verify: self.ssl_verify,
            context_window: self.context_window,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            connect_timeout_seconds: self.connect_timeout_seconds,
            capabilities: self.capabilities.clone(),
            fallback_policy_id: self.fallback_policy_id.clone(),
            fallback_priority: self.fallback_priority,
            catalog_provider_id: self.catalog_provider_id.clone(),
            catalog_provider_name: self.catalog_provider_name.clone(),
            catalog_model_name: self.catalog_model_name.clone(),
            is_default,
            source: self.source.clone(),
        }
    }
}

/// Redacted list response for all configured model profiles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfilesResponse {
    pub loaded: bool,
    pub default_profile: Option<String>,
    pub profiles: Vec<ModelProfileView>,
    pub error: Option<String>,
}

/// Small runtime summary embedded in project status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfileRuntimeSummary {
    pub loaded: bool,
    pub profile_count: usize,
    pub default_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredProfileFile {
    default_profile: Option<String>,
    profiles: BTreeMap<String, StoredModelProfile>,
}

/// Built-in fallback policy strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFallbackStrategy {
    SameProviderThenOtherProvider,
    OtherProviderOnly,
}

/// Model fallback policy used after retryable provider failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFallbackPolicy {
    pub policy_id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub strategy: ModelFallbackStrategy,
    pub max_hops: u32,
    pub cooldown_seconds: u32,
}

/// Fallback config returned by Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFallbackConfig {
    pub policies: Vec<ModelFallbackPolicy>,
}

/// Request for profile-aware model connectivity checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelConnectivityProbeRequest {
    pub profile_name: Option<String>,
    pub override_config: Option<ModelProfileSaveRequest>,
    pub timeout_ms: Option<u64>,
}

/// Request for profile-aware model discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDiscoveryRequest {
    pub profile_name: Option<String>,
    pub override_config: Option<ModelProfileSaveRequest>,
    pub timeout_ms: Option<u64>,
}

/// Token counts reported by providers that include usage metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConnectivityTokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Provider connectivity diagnostics safe for Web display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConnectivityDiagnostics {
    pub endpoint_reachable: bool,
    pub auth_valid: bool,
    pub rate_limited: bool,
}

/// Result of a provider probe request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelConnectivityProbeResult {
    pub ok: bool,
    pub provider: ModelProviderKind,
    pub model: String,
    pub latency_ms: u64,
    pub checked_at_ms: u64,
    pub diagnostics: ModelConnectivityDiagnostics,
    pub token_usage: Option<ModelConnectivityTokenUsage>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub retryable: bool,
}

/// Discovered provider model with optional metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDiscoveryEntry {
    pub model: String,
    pub context_window: Option<u32>,
    pub output_limit: Option<u32>,
    pub capabilities: ModelCapabilities,
}

/// Result of a model discovery request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDiscoveryResult {
    pub ok: bool,
    pub provider: ModelProviderKind,
    pub base_url: String,
    pub latency_ms: u64,
    pub checked_at_ms: u64,
    pub diagnostics: ModelConnectivityDiagnostics,
    pub models: Vec<String>,
    pub model_entries: Vec<ModelDiscoveryEntry>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub retryable: bool,
}

/// Public model catalog provider entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCatalogProvider {
    pub id: String,
    pub name: String,
    pub runtime_provider: ModelProviderKind,
    pub api: Option<String>,
    pub doc: Option<String>,
    pub env: Vec<String>,
    pub models: Vec<ModelCatalogModel>,
}

/// Public model catalog model entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCatalogModel {
    pub id: String,
    pub name: String,
    pub family: Option<String>,
    pub context_window: Option<u32>,
    pub output_limit: Option<u32>,
    pub capabilities: ModelCapabilities,
}

/// Catalog fetch result with cache provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCatalogResult {
    pub ok: bool,
    pub source_url: String,
    pub fetched_at_ms: Option<u64>,
    pub cache_age_seconds: Option<u64>,
    pub stale: bool,
    pub providers: Vec<ModelCatalogProvider>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ModelCatalogCache {
    source_url: String,
    fetched_at_ms: u64,
    providers: Vec<ModelCatalogProvider>,
}

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

    pub async fn profiles(
        &self,
        retrieval: &ReadModelBackendConfig,
    ) -> Result<ModelProfilesResponse, ModelProviderError> {
        let file = self.load_profile_file().await?;
        Ok(profile_response(file, retrieval))
    }

    pub async fn profile_summary(
        &self,
        retrieval: &ReadModelBackendConfig,
    ) -> ModelProfileRuntimeSummary {
        match self.profiles(retrieval).await {
            Ok(response) => ModelProfileRuntimeSummary {
                loaded: response.loaded,
                profile_count: response.profiles.len(),
                default_profile: response.default_profile,
                error: response.error,
            },
            Err(error) => ModelProfileRuntimeSummary {
                loaded: false,
                profile_count: 0,
                default_profile: None,
                error: Some(error.to_string()),
            },
        }
    }

    pub async fn save_profile(
        &self,
        name: &str,
        request: ModelProfileSaveRequest,
        retrieval: &ReadModelBackendConfig,
    ) -> Result<ModelProfilesResponse, ModelProviderError> {
        let name = validate_profile_name(name)?;
        let mut file = self
            .load_profile_file()
            .await?
            .unwrap_or_else(|| StoredProfileFile {
                default_profile: None,
                profiles: BTreeMap::new(),
            });
        let existing = file.profiles.get(&name);
        let is_default = request.is_default || file.default_profile.is_none();
        let stored = StoredModelProfile::from_save_request(request, existing)?;
        file.profiles.insert(name.clone(), stored);
        if is_default {
            file.default_profile = Some(name);
            for (profile_name, profile) in &mut file.profiles {
                profile.is_default = file.default_profile.as_ref() == Some(profile_name);
            }
        }
        self.write_profile_file(&file).await?;
        Ok(profile_response(Some(file), retrieval))
    }

    pub async fn delete_profile(
        &self,
        name: &str,
        retrieval: &ReadModelBackendConfig,
    ) -> Result<ModelProfilesResponse, ModelProviderError> {
        let name = validate_profile_name(name)?;
        let mut file = self
            .load_profile_file()
            .await?
            .unwrap_or_else(|| StoredProfileFile {
                default_profile: None,
                profiles: BTreeMap::new(),
            });
        file.profiles.remove(&name);
        if file.default_profile.as_deref() == Some(&name) {
            file.default_profile = file.profiles.keys().next().cloned();
        }
        for (profile_name, profile) in &mut file.profiles {
            profile.is_default = file.default_profile.as_ref() == Some(profile_name);
        }
        self.write_profile_file(&file).await?;
        Ok(profile_response(Some(file), retrieval))
    }

    pub async fn fallback_config(&self) -> Result<ModelFallbackConfig, ModelProviderError> {
        match fs::read_to_string(self.paths.model_fallback_file()).await {
            Ok(raw) => serde_json::from_str(&raw).map_err(ModelProviderError::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default_fallback()),
            Err(error) => Err(ModelProviderError::from(error)),
        }
    }

    pub async fn save_fallback_config(
        &self,
        config: ModelFallbackConfig,
    ) -> Result<ModelFallbackConfig, ModelProviderError> {
        validate_fallback_config(&config)?;
        write_json(self.paths.model_fallback_file(), &config).await?;
        Ok(config)
    }

    pub async fn catalog(
        &self,
        http: &HttpConfig,
        refresh: bool,
    ) -> Result<ModelCatalogResult, ModelProviderError> {
        let cached = self.load_catalog_cache().await?;
        if !refresh {
            return Ok(cached
                .map(|cache| catalog_result_from_cache(cache, true, None, None))
                .unwrap_or_else(builtin_catalog_result));
        }

        let fetched = self.fetch_catalog(http).await;
        match fetched {
            Ok(result) if result.ok => {
                let cache = ModelCatalogCache {
                    source_url: result.source_url.clone(),
                    fetched_at_ms: result.fetched_at_ms.unwrap_or_else(now_millis),
                    providers: result.providers.clone(),
                };
                let _ = self.write_catalog_cache(&cache).await;
                Ok(result)
            }
            Ok(result) => {
                let fallback_error_code = result.error_code.clone();
                let fallback_error_message = result.error_message.clone();
                let source_url = result.source_url.clone();
                let fetched_at_ms = result.fetched_at_ms;
                Ok(cached
                    .map(|cache| {
                        catalog_result_from_cache(
                            cache,
                            false,
                            fallback_error_code.clone(),
                            fallback_error_message.clone(),
                        )
                    })
                    .unwrap_or_else(|| ModelCatalogResult {
                        ok: false,
                        source_url,
                        fetched_at_ms,
                        cache_age_seconds: None,
                        stale: true,
                        providers: builtin_catalog_providers(),
                        error_code: fallback_error_code,
                        error_message: fallback_error_message,
                    }))
            }
            Err(error) => Ok(cached
                .map(|cache| {
                    catalog_result_from_cache(
                        cache,
                        false,
                        Some("network_error".to_owned()),
                        Some(error.to_string()),
                    )
                })
                .unwrap_or_else(|| ModelCatalogResult {
                    ok: false,
                    source_url: self.catalog_source_url.clone(),
                    fetched_at_ms: None,
                    cache_age_seconds: None,
                    stale: true,
                    providers: builtin_catalog_providers(),
                    error_code: Some("network_error".to_owned()),
                    error_message: Some(error.to_string()),
                })),
        }
    }

    pub async fn probe(
        &self,
        http: &HttpConfig,
        retrieval: &ReadModelBackendConfig,
        request: ModelConnectivityProbeRequest,
    ) -> Result<ModelConnectivityProbeResult, ModelProviderError> {
        let profile = self
            .resolve_probe_profile(retrieval, request.profile_name, request.override_config)
            .await?;
        let request_timeout = request_timeout_from_ms(request.timeout_ms);
        let started = Instant::now();
        let checked_at_ms = now_millis();
        if profile.provider == ModelProviderKind::Echo {
            return Ok(ModelConnectivityProbeResult {
                ok: true,
                provider: profile.provider,
                model: profile.model,
                latency_ms: elapsed_millis(started),
                checked_at_ms,
                diagnostics: ok_diagnostics(),
                token_usage: Some(ModelConnectivityTokenUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                }),
                error_code: None,
                error_message: None,
                retryable: false,
            });
        }
        if matches!(
            profile.provider,
            ModelProviderKind::Maas | ModelProviderKind::Codeagent
        ) {
            return Ok(unsupported_probe(profile, started, checked_at_ms));
        }

        let client = provider_http_client(http, &profile)?;
        let response = send_probe_request(&client, &profile, request_timeout).await;
        Ok(probe_result_from_http(profile, started, checked_at_ms, response).await)
    }

    pub async fn discover(
        &self,
        http: &HttpConfig,
        retrieval: &ReadModelBackendConfig,
        request: ModelDiscoveryRequest,
    ) -> Result<ModelDiscoveryResult, ModelProviderError> {
        let profile = self
            .resolve_probe_profile(retrieval, request.profile_name, request.override_config)
            .await?;
        let request_timeout = request_timeout_from_ms(request.timeout_ms);
        let started = Instant::now();
        let checked_at_ms = now_millis();
        if profile.provider == ModelProviderKind::Echo {
            return Ok(ModelDiscoveryResult {
                ok: true,
                provider: profile.provider,
                base_url: redacted_url(&profile.base_url),
                latency_ms: elapsed_millis(started),
                checked_at_ms,
                diagnostics: ok_diagnostics(),
                models: vec![profile.model.clone()],
                model_entries: vec![ModelDiscoveryEntry {
                    model: profile.model,
                    context_window: None,
                    output_limit: None,
                    capabilities: ModelCapabilities::default(),
                }],
                error_code: None,
                error_message: None,
                retryable: false,
            });
        }
        if matches!(
            profile.provider,
            ModelProviderKind::Maas | ModelProviderKind::Codeagent
        ) {
            return Ok(unsupported_discovery(profile, started, checked_at_ms));
        }

        let client = provider_http_client(http, &profile)?;
        let response = send_discovery_request(&client, &profile, request_timeout).await;
        Ok(discovery_result_from_http(profile, started, checked_at_ms, response).await)
    }

    async fn resolve_probe_profile(
        &self,
        retrieval: &ReadModelBackendConfig,
        profile_name: Option<String>,
        override_config: Option<ModelProfileSaveRequest>,
    ) -> Result<StoredModelProfile, ModelProviderError> {
        match (profile_name, override_config) {
            (Some(name), Some(request)) => {
                let base = self.resolve_profile_by_name(retrieval, &name).await?;
                StoredModelProfile::from_save_request(request, Some(&base))
            }
            (Some(name), None) => self.resolve_profile_by_name(retrieval, &name).await,
            (None, Some(request)) => {
                let base = match self.resolve_default_profile(retrieval).await {
                    Ok(profile) => Some(profile),
                    Err(ModelProviderError::InvalidInput(message))
                        if message == "no model profile is configured" =>
                    {
                        None
                    }
                    Err(error) => return Err(error),
                };
                StoredModelProfile::from_save_request(request, base.as_ref())
            }
            (None, None) => self.resolve_default_profile(retrieval).await,
        }
    }

    async fn resolve_default_profile(
        &self,
        retrieval: &ReadModelBackendConfig,
    ) -> Result<StoredModelProfile, ModelProviderError> {
        let file = self.load_profile_file().await?;
        let response = profile_response(file.clone(), retrieval);
        let Some(default_name) = response.default_profile else {
            return Err(ModelProviderError::InvalidInput(
                "no model profile is configured".to_owned(),
            ));
        };
        self.resolve_profile_by_name(retrieval, &default_name).await
    }

    async fn resolve_profile_by_name(
        &self,
        retrieval: &ReadModelBackendConfig,
        name: &str,
    ) -> Result<StoredModelProfile, ModelProviderError> {
        let name = validate_profile_name(name)?;
        if let Some(file) = self.load_profile_file().await? {
            if let Some(profile) = file.profiles.get(&name) {
                return Ok(profile.clone());
            }
        }
        if name == DEFAULT_PROFILE_NAME {
            if let Some(profile) = StoredModelProfile::from_runtime(retrieval) {
                return Ok(profile);
            }
        }
        Err(ModelProviderError::InvalidInput(format!(
            "model profile '{name}' was not found"
        )))
    }

    async fn load_profile_file(&self) -> Result<Option<StoredProfileFile>, ModelProviderError> {
        match fs::read_to_string(self.paths.model_profiles_file()).await {
            Ok(raw) => serde_json::from_str(&raw)
                .map(Some)
                .map_err(ModelProviderError::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ModelProviderError::from(error)),
        }
    }

    async fn write_profile_file(&self, file: &StoredProfileFile) -> Result<(), ModelProviderError> {
        write_json(self.paths.model_profiles_file(), file).await
    }

    async fn load_catalog_cache(&self) -> Result<Option<ModelCatalogCache>, ModelProviderError> {
        match fs::read_to_string(self.paths.model_catalog_cache_file()).await {
            Ok(raw) => serde_json::from_str(&raw)
                .map(Some)
                .map_err(ModelProviderError::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ModelProviderError::from(error)),
        }
    }

    async fn write_catalog_cache(
        &self,
        cache: &ModelCatalogCache,
    ) -> Result<(), ModelProviderError> {
        write_json(self.paths.model_catalog_cache_file(), cache).await
    }

    async fn fetch_catalog(
        &self,
        http: &HttpConfig,
    ) -> Result<ModelCatalogResult, ModelProviderError> {
        let client = crate::net::http::outbound_json_client(http)
            .map_err(|error| ModelProviderError::Network(error.to_string()))?;
        let response = client
            .get(&self.catalog_source_url)
            .timeout(http.request_timeout)
            .send()
            .await
            .map_err(|error| ModelProviderError::Network(error.to_string()))?;
        if !response.status().is_success() {
            return Ok(ModelCatalogResult {
                ok: false,
                source_url: self.catalog_source_url.clone(),
                fetched_at_ms: None,
                cache_age_seconds: None,
                stale: true,
                providers: Vec::new(),
                error_code: Some(status_error_code(response.status().as_u16()).to_owned()),
                error_message: Some(format!("catalog returned HTTP {}", response.status())),
            });
        }
        let payload = response
            .json::<Value>()
            .await
            .map_err(|error| ModelProviderError::Json(error.to_string()))?;
        Ok(ModelCatalogResult {
            ok: true,
            source_url: self.catalog_source_url.clone(),
            fetched_at_ms: Some(now_millis()),
            cache_age_seconds: Some(0),
            stale: false,
            providers: parse_catalog_payload(&payload),
            error_code: None,
            error_message: None,
        })
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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
