//! Owns provider HTTP requests, protocol responses, and connectivity diagnostics.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

use super::*;
use crate::net::{
    http::{QosHttpClientError, QosHttpResponse, send_request_with_qos},
    qos::{QosPolicy, QosRuntime},
};

#[cfg(test)]
pub(super) async fn send_probe_request(
    client: &reqwest::Client,
    profile: &StoredModelProfile,
    request_timeout: Option<Duration>,
) -> Result<QosHttpResponse, QosHttpClientError> {
    let qos = QosRuntime::default();
    let policy = default_test_qos_policy();
    send_probe_request_with_qos(client, &qos, &policy, profile, request_timeout).await
}

pub(super) async fn send_probe_request_with_qos(
    client: &reqwest::Client,
    qos: &QosRuntime,
    policy: &QosPolicy,
    profile: &StoredModelProfile,
    request_timeout: Option<Duration>,
) -> Result<QosHttpResponse, QosHttpClientError> {
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
        ModelProviderKind::OpenAiCompatible if uses_embedding_probe(profile) => client
            .post(format!(
                "{}/embeddings",
                profile.base_url.trim_end_matches('/')
            ))
            .headers(auth_headers(profile))
            .json(&json!({
                "model": profile.model,
                "input": "relay-knowledge provider probe"
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
    send_request_with_qos(qos, policy, apply_request_timeout(request, request_timeout)).await
}

fn uses_embedding_probe(profile: &StoredModelProfile) -> bool {
    profile.source == "environment" || is_embedding_model_name(&profile.model)
}

fn is_embedding_model_name(model: &str) -> bool {
    let normalized = model.to_ascii_lowercase();
    normalized.contains("embedding")
        || normalized.contains("embed")
        || normalized.starts_with("bge-")
        || normalized.starts_with("e5-")
}

#[cfg(test)]
pub(super) async fn send_discovery_request(
    client: &reqwest::Client,
    profile: &StoredModelProfile,
    request_timeout: Option<Duration>,
) -> Result<QosHttpResponse, QosHttpClientError> {
    let qos = QosRuntime::default();
    let policy = default_test_qos_policy();
    send_discovery_request_with_qos(client, &qos, &policy, profile, request_timeout).await
}

pub(super) async fn send_discovery_request_with_qos(
    client: &reqwest::Client,
    qos: &QosRuntime,
    policy: &QosPolicy,
    profile: &StoredModelProfile,
    request_timeout: Option<Duration>,
) -> Result<QosHttpResponse, QosHttpClientError> {
    let url = match profile.provider {
        ModelProviderKind::Anthropic => {
            format!("{}/v1/models", profile.base_url.trim_end_matches('/'))
        }
        _ => format!("{}/models", profile.base_url.trim_end_matches('/')),
    };
    send_request_with_qos(
        qos,
        policy,
        apply_request_timeout(
            client.get(url).headers(auth_headers(profile)),
            request_timeout,
        ),
    )
    .await
}

#[cfg(test)]
fn default_test_qos_policy() -> QosPolicy {
    QosPolicy::new(
        crate::net::qos::DEFAULT_MAX_CONNECTIONS,
        crate::net::qos::DEFAULT_MAX_IN_FLIGHT_REQUESTS,
        crate::net::qos::DEFAULT_MAX_QUEUE_DEPTH,
    )
    .expect("default QoS policy should validate")
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
    response: Result<QosHttpResponse, QosHttpClientError>,
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
    response: Result<QosHttpResponse, QosHttpClientError>,
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
    error: QosHttpClientError,
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
