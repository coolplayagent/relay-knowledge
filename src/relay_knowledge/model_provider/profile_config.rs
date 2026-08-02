//! Owns profile normalization, validation, and response projection.

use std::collections::BTreeSet;

use reqwest::header::{HeaderName, HeaderValue};

use super::profile::{
    DEFAULT_PROFILE_NAME, StoredModelProfile, StoredProfileFile, default_connect_timeout_seconds,
    default_temperature, default_top_p,
};
use super::*;
use crate::retrieval::{EmbeddingProviderKind, ReadModelBackendConfig};

impl ModelRequestHeader {
    pub(super) fn normalized(mut self) -> Result<Self, ModelProviderError> {
        self.name = non_empty_string(self.name, "header name")?;
        self.value = self
            .value
            .and_then(|value| non_empty_string(value, "header value").ok());
        self.configured = self.configured || self.value.is_some();
        Ok(self)
    }
}

impl StoredModelProfile {
    pub(super) fn from_save_request(
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
            ssl_verify: request
                .ssl_verify
                .or_else(|| existing.and_then(|profile| profile.ssl_verify)),
            context_window: request.context_window,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            top_p: request.top_p,
            connect_timeout_seconds: request.connect_timeout_seconds,
            capabilities: request.capabilities.unwrap_or_else(|| {
                existing
                    .map(|profile| profile.capabilities.clone())
                    .unwrap_or_default()
            }),
            fallback_policy_id: request.fallback_policy_id.and_then(normalize_optional),
            fallback_priority: request.fallback_priority,
            catalog_provider_id: request.catalog_provider_id.and_then(normalize_optional),
            catalog_provider_name: request.catalog_provider_name.and_then(normalize_optional),
            catalog_model_name: request.catalog_model_name.and_then(normalize_optional),
            is_default: request.is_default,
            source: "config".to_owned(),
        })
    }

    pub(super) fn from_runtime(retrieval: &ReadModelBackendConfig) -> Option<Self> {
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
}

pub(super) fn profile_response(
    file: Option<StoredProfileFile>,
    retrieval: &ReadModelBackendConfig,
) -> ModelProfilesResponse {
    let mut profiles = file
        .as_ref()
        .map(|stored| stored.profiles.clone())
        .unwrap_or_default();
    if profiles.is_empty() {
        if let Some(runtime_profile) = StoredModelProfile::from_runtime(retrieval) {
            profiles.insert(DEFAULT_PROFILE_NAME.to_owned(), runtime_profile);
        }
    }
    let default_profile = file
        .and_then(|stored| stored.default_profile)
        .or_else(|| profiles.keys().next().cloned());
    let views = profiles
        .iter()
        .map(|(name, profile)| {
            let is_default = default_profile.as_ref() == Some(name) || profile.is_default;
            profile.to_view(name, is_default)
        })
        .collect();

    ModelProfilesResponse {
        loaded: true,
        default_profile,
        profiles: views,
        error: None,
    }
}

pub(super) fn runtime_profile_merge_base(
    file: &StoredProfileFile,
    name: &str,
    retrieval: &ReadModelBackendConfig,
) -> Option<StoredModelProfile> {
    if !file.profiles.is_empty() || file.default_profile.is_some() || name != DEFAULT_PROFILE_NAME {
        return None;
    }
    StoredModelProfile::from_runtime(retrieval)
}

pub(super) fn validate_headers(
    headers: Vec<ModelRequestHeader>,
    existing: Option<&[ModelRequestHeader]>,
) -> Result<Vec<ModelRequestHeader>, ModelProviderError> {
    let mut names = BTreeSet::new();
    headers
        .into_iter()
        .map(ModelRequestHeader::normalized)
        .map(|result| {
            let mut header = result?;
            let folded = header.name.to_ascii_lowercase();
            if !names.insert(folded) {
                return Err(ModelProviderError::InvalidInput(format!(
                    "duplicate model header '{}'",
                    header.name
                )));
            }
            HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| {
                ModelProviderError::InvalidInput(format!(
                    "invalid model header name '{}'",
                    header.name
                ))
            })?;
            match header.value.as_ref() {
                Some(_) => {}
                None if header.configured => {
                    if let Some(value) = existing_header_value(existing, &header.name) {
                        header.value = Some(value);
                    } else {
                        return Err(ModelProviderError::InvalidInput(format!(
                            "model header '{}' requires a value",
                            header.name
                        )));
                    }
                }
                None => {}
            }
            if let Some(value) = header.value.as_ref() {
                HeaderValue::from_str(value).map_err(|_| {
                    ModelProviderError::InvalidInput(format!(
                        "invalid model header value for '{}'",
                        header.name
                    ))
                })?;
            }
            Ok(header)
        })
        .collect()
}

fn existing_header_value(existing: Option<&[ModelRequestHeader]>, name: &str) -> Option<String> {
    existing?
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .and_then(|header| header.value.clone())
}

pub(super) fn normalized_base_url(
    provider: ModelProviderKind,
    value: Option<String>,
) -> Result<String, ModelProviderError> {
    let candidate = value
        .and_then(normalize_optional)
        .or_else(|| provider.default_base_url().map(ToOwned::to_owned));
    let Some(base_url) = candidate else {
        return Err(ModelProviderError::InvalidInput(
            "base_url is required for this provider".to_owned(),
        ));
    };
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err(ModelProviderError::InvalidInput(
            "base_url must use http:// or https://".to_owned(),
        ));
    }
    Ok(base_url.trim_end_matches('/').to_owned())
}

pub(super) fn provider_allows_missing_auth(provider: ModelProviderKind) -> bool {
    matches!(
        provider,
        ModelProviderKind::Echo | ModelProviderKind::Maas | ModelProviderKind::Codeagent
    )
}

pub(super) fn validate_sampling(
    temperature: f64,
    top_p: f64,
    timeout: f64,
) -> Result<(), ModelProviderError> {
    if !(0.0..=2.0).contains(&temperature) {
        return Err(ModelProviderError::InvalidInput(
            "temperature must be between 0 and 2".to_owned(),
        ));
    }
    if !(0.0..=1.0).contains(&top_p) {
        return Err(ModelProviderError::InvalidInput(
            "top_p must be between 0 and 1".to_owned(),
        ));
    }
    if timeout <= 0.0 || timeout > 300.0 {
        return Err(ModelProviderError::InvalidInput(
            "connect_timeout_seconds must be between 0 and 300".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_profile_name(name: &str) -> Result<String, ModelProviderError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.len() > 80
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(ModelProviderError::InvalidInput(
            "profile name must contain only letters, numbers, '.', '-', or '_'".to_owned(),
        ));
    }
    Ok(trimmed.to_owned())
}

pub(super) fn non_empty_string(
    value: String,
    field: &'static str,
) -> Result<String, ModelProviderError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ModelProviderError::InvalidInput(format!(
            "{field} must not be empty"
        )));
    }
    Ok(trimmed.to_owned())
}

pub(super) fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
#[path = "profile_config_tests.rs"]
mod tests;
