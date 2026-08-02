//! Owns public model profile contracts and their persisted representation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_PROFILE_NAME: &str = "default";
const DEFAULT_CONNECT_TIMEOUT_SECONDS: f64 = 30.0;
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_CODEAGENT_BASE_URL: &str = "https://codeagentcli.rnd.huawei.com/codeAgentPro";
const DEFAULT_MAAS_BASE_URL: &str =
    "http://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2/";

pub(super) fn default_temperature() -> f64 {
    0.7
}

pub(super) fn default_top_p() -> f64 {
    1.0
}

pub(super) fn default_connect_timeout_seconds() -> f64 {
    DEFAULT_CONNECT_TIMEOUT_SECONDS
}

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

    pub(super) const fn default_base_url(self) -> Option<&'static str> {
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
pub(super) struct StoredModelProfile {
    pub(super) provider: ModelProviderKind,
    pub(super) model: String,
    pub(super) base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) api_key: Option<String>,
    #[serde(default)]
    pub(super) headers: Vec<ModelRequestHeader>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ssl_verify: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) max_tokens: Option<u32>,
    pub(super) temperature: f64,
    pub(super) top_p: f64,
    pub(super) connect_timeout_seconds: f64,
    #[serde(default)]
    pub(super) capabilities: ModelCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) fallback_policy_id: Option<String>,
    pub(super) fallback_priority: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) catalog_provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) catalog_provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) catalog_model_name: Option<String>,
    #[serde(default)]
    pub(super) is_default: bool,
    pub(super) source: String,
}

impl StoredModelProfile {
    pub(super) fn to_view(&self, name: &str, is_default: bool) -> ModelProfileView {
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

pub(super) fn redacted_url(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_owned();
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, suffix) = rest.split_at(authority_end);
    authority
        .rsplit_once('@')
        .map(|(_, host)| format!("{scheme}://{host}{suffix}"))
        .unwrap_or_else(|| value.to_owned())
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
pub(super) struct StoredProfileFile {
    pub(super) default_profile: Option<String>,
    pub(super) profiles: BTreeMap<String, StoredModelProfile>,
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
