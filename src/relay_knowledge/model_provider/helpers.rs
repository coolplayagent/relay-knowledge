use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::fs;

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

pub(super) fn default_fallback() -> ModelFallbackConfig {
    ModelFallbackConfig {
        policies: vec![
            ModelFallbackPolicy {
                policy_id: "same_provider_then_other_provider".to_owned(),
                name: "Same Provider Then Other Provider".to_owned(),
                description: "Retry same-provider alternatives before switching providers."
                    .to_owned(),
                enabled: true,
                strategy: ModelFallbackStrategy::SameProviderThenOtherProvider,
                max_hops: 3,
                cooldown_seconds: 60,
            },
            ModelFallbackPolicy {
                policy_id: "other_provider_only".to_owned(),
                name: "Other Provider Only".to_owned(),
                description: "Fail over directly to profiles from other providers.".to_owned(),
                enabled: true,
                strategy: ModelFallbackStrategy::OtherProviderOnly,
                max_hops: 3,
                cooldown_seconds: 60,
            },
        ],
    }
}

pub(super) fn validate_fallback_config(
    config: &ModelFallbackConfig,
) -> Result<(), ModelProviderError> {
    let mut ids = BTreeSet::new();
    for policy in &config.policies {
        let id = validate_profile_name(&policy.policy_id)?;
        if !ids.insert(id.clone()) {
            return Err(ModelProviderError::InvalidInput(format!(
                "duplicate fallback policy id '{id}'"
            )));
        }
        if policy.max_hops == 0 || policy.cooldown_seconds > 3600 {
            return Err(ModelProviderError::InvalidInput(
                "fallback policy max_hops must be positive and cooldown_seconds <= 3600".to_owned(),
            ));
        }
    }
    Ok(())
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

pub(super) async fn write_json<T: Serialize>(
    path: PathBuf,
    value: &T,
) -> Result<(), ModelProviderError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let body = serde_json::to_vec_pretty(value)?;
    fs::write(path, body)
        .await
        .map_err(ModelProviderError::from)
}

pub(super) async fn send_probe_request(
    client: &reqwest::Client,
    profile: &StoredModelProfile,
    request_timeout: Option<Duration>,
) -> Result<reqwest::Response, reqwest::Error> {
    let request = match profile.provider {
        ModelProviderKind::Anthropic => client
            .post(format!(
                "{}/v1/messages",
                profile.base_url.trim_end_matches('/')
            ))
            .headers(auth_headers(profile))
            .json(&json!({
                "model": profile.model,
                "max_tokens": profile.max_tokens.unwrap_or(16),
                "messages": [{"role": "user", "content": "relay-knowledge provider probe"}]
            })),
        _ => client
            .post(format!(
                "{}/chat/completions",
                profile.base_url.trim_end_matches('/')
            ))
            .headers(auth_headers(profile))
            .json(&json!({
                "model": profile.model,
                "temperature": profile.temperature,
                "top_p": profile.top_p,
                "max_tokens": profile.max_tokens.unwrap_or(16),
                "messages": [{"role": "user", "content": "relay-knowledge provider probe"}]
            })),
    };
    apply_request_timeout(request, request_timeout).send().await
}

pub(super) async fn send_discovery_request(
    client: &reqwest::Client,
    profile: &StoredModelProfile,
    request_timeout: Option<Duration>,
) -> Result<reqwest::Response, reqwest::Error> {
    let url = match profile.provider {
        ModelProviderKind::Anthropic => {
            format!("{}/v1/models", profile.base_url.trim_end_matches('/'))
        }
        _ => format!("{}/models", profile.base_url.trim_end_matches('/')),
    };
    apply_request_timeout(
        client.get(url).headers(auth_headers(profile)),
        request_timeout,
    )
    .send()
    .await
}

pub(super) fn provider_http_client(
    http: &HttpConfig,
    profile: &StoredModelProfile,
) -> Result<reqwest::Client, ModelProviderError> {
    crate::net::http::outbound_json_client_with_policy(
        http,
        profile.ssl_verify,
        Some(Duration::from_secs_f64(profile.connect_timeout_seconds)),
    )
    .map_err(|error| ModelProviderError::Network(error.to_string()))
}

fn apply_request_timeout(
    request: reqwest::RequestBuilder,
    timeout: Option<Duration>,
) -> reqwest::RequestBuilder {
    match timeout {
        Some(timeout) => request.timeout(timeout),
        None => request,
    }
}

pub(super) fn auth_headers(profile: &StoredModelProfile) -> HeaderMap {
    let mut headers = HeaderMap::new();
    match profile.provider {
        ModelProviderKind::Anthropic => {
            if let Some(api_key) = &profile.api_key {
                if let Ok(value) = HeaderValue::from_str(api_key) {
                    headers.insert("x-api-key", value);
                }
            }
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        _ => {
            if let Some(api_key) = &profile.api_key {
                if let Ok(value) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
                    headers.insert("authorization", value);
                }
            }
        }
    }
    for header in &profile.headers {
        if let (Ok(name), Some(value)) = (
            HeaderName::from_bytes(header.name.as_bytes()),
            header.value.as_ref(),
        ) {
            if let Ok(value) = HeaderValue::from_str(value) {
                headers.insert(name, value);
            }
        }
    }
    headers
}

pub(super) async fn probe_result_from_http(
    profile: StoredModelProfile,
    started: Instant,
    checked_at_ms: u64,
    response: Result<reqwest::Response, reqwest::Error>,
) -> ModelConnectivityProbeResult {
    match response {
        Ok(response) => {
            let status = response.status();
            let token_usage = response
                .json::<Value>()
                .await
                .ok()
                .and_then(|payload| token_usage(&payload));
            let ok = status.is_success();
            ModelConnectivityProbeResult {
                ok,
                provider: profile.provider,
                model: profile.model,
                latency_ms: elapsed_millis(started),
                checked_at_ms,
                diagnostics: diagnostics_from_status(status.as_u16()),
                token_usage,
                error_code: (!ok).then(|| status_error_code(status.as_u16()).to_owned()),
                error_message: (!ok).then(|| format!("provider returned HTTP {status}")),
                retryable: is_retryable_status(status.as_u16()),
            }
        }
        Err(error) => transport_probe_result(profile, started, checked_at_ms, error),
    }
}

pub(super) async fn discovery_result_from_http(
    profile: StoredModelProfile,
    started: Instant,
    checked_at_ms: u64,
    response: Result<reqwest::Response, reqwest::Error>,
) -> ModelDiscoveryResult {
    match response {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                return ModelDiscoveryResult {
                    ok: false,
                    provider: profile.provider,
                    base_url: redacted_url(&profile.base_url),
                    latency_ms: elapsed_millis(started),
                    checked_at_ms,
                    diagnostics: diagnostics_from_status(status.as_u16()),
                    models: Vec::new(),
                    model_entries: Vec::new(),
                    error_code: Some(status_error_code(status.as_u16()).to_owned()),
                    error_message: Some(format!("provider returned HTTP {status}")),
                    retryable: is_retryable_status(status.as_u16()),
                };
            }
            let payload = match response.json::<Value>().await {
                Ok(payload) => payload,
                Err(error) => {
                    return ModelDiscoveryResult {
                        ok: false,
                        provider: profile.provider,
                        base_url: redacted_url(&profile.base_url),
                        latency_ms: elapsed_millis(started),
                        checked_at_ms,
                        diagnostics: ModelConnectivityDiagnostics {
                            endpoint_reachable: true,
                            auth_valid: true,
                            rate_limited: false,
                        },
                        models: Vec::new(),
                        model_entries: Vec::new(),
                        error_code: Some("invalid_response".to_owned()),
                        error_message: Some(format!(
                            "provider returned invalid model discovery JSON: {error}"
                        )),
                        retryable: false,
                    };
                }
            };
            let entries = parse_discovery_entries(&payload);
            let models = entries.iter().map(|entry| entry.model.clone()).collect();
            ModelDiscoveryResult {
                ok: true,
                provider: profile.provider,
                base_url: redacted_url(&profile.base_url),
                latency_ms: elapsed_millis(started),
                checked_at_ms,
                diagnostics: ok_diagnostics(),
                models,
                model_entries: entries,
                error_code: None,
                error_message: None,
                retryable: false,
            }
        }
        Err(error) => ModelDiscoveryResult {
            ok: false,
            provider: profile.provider,
            base_url: redacted_url(&profile.base_url),
            latency_ms: elapsed_millis(started),
            checked_at_ms,
            diagnostics: ModelConnectivityDiagnostics {
                endpoint_reachable: false,
                auth_valid: false,
                rate_limited: false,
            },
            models: Vec::new(),
            model_entries: Vec::new(),
            error_code: Some(if error.is_timeout() {
                "network_timeout".to_owned()
            } else {
                "network_error".to_owned()
            }),
            error_message: Some(error.to_string()),
            retryable: true,
        },
    }
}

pub(super) fn transport_probe_result(
    profile: StoredModelProfile,
    started: Instant,
    checked_at_ms: u64,
    error: reqwest::Error,
) -> ModelConnectivityProbeResult {
    ModelConnectivityProbeResult {
        ok: false,
        provider: profile.provider,
        model: profile.model,
        latency_ms: elapsed_millis(started),
        checked_at_ms,
        diagnostics: ModelConnectivityDiagnostics {
            endpoint_reachable: false,
            auth_valid: false,
            rate_limited: false,
        },
        token_usage: None,
        error_code: Some(if error.is_timeout() {
            "network_timeout".to_owned()
        } else {
            "network_error".to_owned()
        }),
        error_message: Some(error.to_string()),
        retryable: true,
    }
}

pub(super) fn unsupported_probe(
    profile: StoredModelProfile,
    started: Instant,
    checked_at_ms: u64,
) -> ModelConnectivityProbeResult {
    ModelConnectivityProbeResult {
        ok: false,
        provider: profile.provider,
        model: profile.model,
        latency_ms: elapsed_millis(started),
        checked_at_ms,
        diagnostics: ModelConnectivityDiagnostics {
            endpoint_reachable: false,
            auth_valid: false,
            rate_limited: false,
        },
        token_usage: None,
        error_code: Some("unsupported_auth_source".to_owned()),
        error_message: Some(
            "this provider requires enterprise auth not configured in relay-knowledge".to_owned(),
        ),
        retryable: false,
    }
}

pub(super) fn unsupported_discovery(
    profile: StoredModelProfile,
    started: Instant,
    checked_at_ms: u64,
) -> ModelDiscoveryResult {
    ModelDiscoveryResult {
        ok: false,
        provider: profile.provider,
        base_url: redacted_url(&profile.base_url),
        latency_ms: elapsed_millis(started),
        checked_at_ms,
        diagnostics: ModelConnectivityDiagnostics {
            endpoint_reachable: false,
            auth_valid: false,
            rate_limited: false,
        },
        models: Vec::new(),
        model_entries: Vec::new(),
        error_code: Some("unsupported_auth_source".to_owned()),
        error_message: Some(
            "this provider requires enterprise auth not configured in relay-knowledge".to_owned(),
        ),
        retryable: false,
    }
}

pub(super) fn token_usage(payload: &Value) -> Option<ModelConnectivityTokenUsage> {
    let usage = payload.get("usage")?;
    Some(ModelConnectivityTokenUsage {
        prompt_tokens: usage
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        completion_tokens: usage
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: usage
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

pub(super) fn parse_discovery_entries(payload: &Value) -> Vec<ModelDiscoveryEntry> {
    payload
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let model = entry
                .get("id")
                .or_else(|| entry.get("name"))
                .and_then(Value::as_str)?;
            Some(ModelDiscoveryEntry {
                model: model.to_owned(),
                context_window: entry
                    .get("context_window")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
                output_limit: entry
                    .get("output_limit")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
                capabilities: ModelCapabilities::default(),
            })
        })
        .collect()
}

pub(super) fn diagnostics_from_status(status: u16) -> ModelConnectivityDiagnostics {
    ModelConnectivityDiagnostics {
        endpoint_reachable: true,
        auth_valid: status != 401 && status != 403,
        rate_limited: status == 429,
    }
}

pub(super) fn status_error_code(status: u16) -> &'static str {
    match status {
        401 | 403 => "auth_failed",
        408 | 504 => "network_timeout",
        429 => "rate_limited",
        500..=599 => "provider_error",
        _ => "http_error",
    }
}

pub(super) fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500..=599)
}

pub(super) fn ok_diagnostics() -> ModelConnectivityDiagnostics {
    ModelConnectivityDiagnostics {
        endpoint_reachable: true,
        auth_valid: true,
        rate_limited: false,
    }
}

pub(super) fn parse_catalog_payload(payload: &Value) -> Vec<ModelCatalogProvider> {
    let providers = payload
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let parsed = providers
        .iter()
        .filter_map(parse_catalog_provider)
        .collect::<Vec<_>>();
    if parsed.is_empty() {
        builtin_catalog_providers()
    } else {
        parsed
    }
}

pub(super) fn parse_catalog_provider(value: &Value) -> Option<ModelCatalogProvider> {
    let id = value.get("id").and_then(Value::as_str)?.to_owned();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_owned();
    let runtime_provider = match value
        .get("runtime_provider")
        .or_else(|| value.get("provider"))
        .and_then(Value::as_str)
        .unwrap_or("openai_compatible")
    {
        "anthropic" => ModelProviderKind::Anthropic,
        "bigmodel" => ModelProviderKind::Bigmodel,
        "minimax" => ModelProviderKind::Minimax,
        "maas" => ModelProviderKind::Maas,
        "codeagent" => ModelProviderKind::Codeagent,
        "echo" => ModelProviderKind::Echo,
        _ => ModelProviderKind::OpenAiCompatible,
    };
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_catalog_model)
        .collect();
    Some(ModelCatalogProvider {
        id,
        name,
        runtime_provider,
        api: value
            .get("api")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        doc: value
            .get("doc")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        env: value
            .get("env")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        models,
    })
}

pub(super) fn parse_catalog_model(value: &Value) -> Option<ModelCatalogModel> {
    let id = value
        .get("id")
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)?
        .to_owned();
    Some(ModelCatalogModel {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_owned(),
        id,
        family: value
            .get("family")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        context_window: value
            .get("context_window")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        output_limit: value
            .get("output_limit")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        capabilities: ModelCapabilities::default(),
    })
}

pub(super) fn builtin_catalog_result() -> ModelCatalogResult {
    ModelCatalogResult {
        ok: true,
        source_url: "builtin".to_owned(),
        fetched_at_ms: Some(now_millis()),
        cache_age_seconds: Some(0),
        stale: false,
        providers: builtin_catalog_providers(),
        error_code: None,
        error_message: None,
    }
}

pub(super) fn builtin_catalog_providers() -> Vec<ModelCatalogProvider> {
    vec![
        catalog_provider(
            "openai",
            "OpenAI-compatible",
            ModelProviderKind::OpenAiCompatible,
            &["gpt-4.1", "gpt-4.1-mini", "text-embedding-3-small"],
        ),
        catalog_provider(
            "anthropic",
            "Anthropic",
            ModelProviderKind::Anthropic,
            &["claude-sonnet-4-5", "claude-haiku-4-5"],
        ),
        catalog_provider("echo", "Echo", ModelProviderKind::Echo, &["echo"]),
    ]
}

pub(super) fn catalog_provider(
    id: &str,
    name: &str,
    runtime_provider: ModelProviderKind,
    models: &[&str],
) -> ModelCatalogProvider {
    ModelCatalogProvider {
        id: id.to_owned(),
        name: name.to_owned(),
        runtime_provider,
        api: None,
        doc: None,
        env: Vec::new(),
        models: models
            .iter()
            .map(|model| ModelCatalogModel {
                id: (*model).to_owned(),
                name: (*model).to_owned(),
                family: None,
                context_window: None,
                output_limit: None,
                capabilities: ModelCapabilities::default(),
            })
            .collect(),
    }
}

pub(super) fn catalog_result_from_cache(
    cache: ModelCatalogCache,
    ok: bool,
    error_code: Option<String>,
    error_message: Option<String>,
) -> ModelCatalogResult {
    let age = now_millis().saturating_sub(cache.fetched_at_ms) / 1000;
    ModelCatalogResult {
        ok,
        source_url: cache.source_url,
        fetched_at_ms: Some(cache.fetched_at_ms),
        cache_age_seconds: Some(age),
        stale: !ok,
        providers: cache.providers,
        error_code,
        error_message,
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

pub(super) fn request_timeout_from_ms(timeout_ms: Option<u64>) -> Option<Duration> {
    timeout_ms.map(Duration::from_millis)
}

pub(super) fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub(super) fn now_millis() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
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

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
