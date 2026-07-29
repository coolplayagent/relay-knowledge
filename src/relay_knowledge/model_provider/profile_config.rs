//! Owns profile normalization, validation, and response projection.

use std::collections::BTreeSet;

use reqwest::header::{HeaderName, HeaderValue};

use super::*;

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

pub(super) fn default_temperature() -> f64 {
    0.7
}

pub(super) fn default_top_p() -> f64 {
    1.0
}

pub(super) fn default_connect_timeout_seconds() -> f64 {
    DEFAULT_CONNECT_TIMEOUT_SECONDS
}
