use std::fmt;

use serde::{Deserialize, Serialize};

use super::{EmbeddingProviderKind, RemoteEmbeddingConfig};
use crate::net::{
    http::{QosHttpClientError, QosHttpResponse, send_request_with_qos},
    qos::{QosPolicy, QosRuntime},
};

pub use crate::ports::embedding::{
    EmbeddingFuture, EmbeddingProvider, EmbeddingProviderError, EmbeddingRequest, EmbeddingVector,
    ProviderRetryClass,
};

const PROVIDER_ERROR_MESSAGE_LIMIT: usize = 240;

/// Builds the configured remote embedding provider.
pub fn embedding_provider(
    config: RemoteEmbeddingConfig,
    client: reqwest::Client,
) -> Box<dyn EmbeddingProvider> {
    embedding_provider_with_optional_qos(config, client, None)
}

/// Builds the configured remote embedding provider with outbound QoS admission.
pub fn embedding_provider_with_qos(
    config: RemoteEmbeddingConfig,
    client: reqwest::Client,
    qos: QosRuntime,
    policy: QosPolicy,
) -> Box<dyn EmbeddingProvider> {
    embedding_provider_with_optional_qos(config, client, Some((qos, policy)))
}

fn embedding_provider_with_optional_qos(
    config: RemoteEmbeddingConfig,
    client: reqwest::Client,
    qos: Option<(QosRuntime, QosPolicy)>,
) -> Box<dyn EmbeddingProvider> {
    match config.provider {
        EmbeddingProviderKind::OpenAiCompatible => Box::new(OpenAiCompatibleEmbeddingProvider {
            config,
            client,
            qos,
        }),
        EmbeddingProviderKind::Echo => Box::new(EchoEmbeddingProvider { config }),
    }
}

struct OpenAiCompatibleEmbeddingProvider {
    config: RemoteEmbeddingConfig,
    client: reqwest::Client,
    qos: Option<(QosRuntime, QosPolicy)>,
}

impl EmbeddingProvider for OpenAiCompatibleEmbeddingProvider {
    fn embed(&self, request: EmbeddingRequest) -> EmbeddingFuture<'_, Vec<EmbeddingVector>> {
        Box::pin(async move {
            validate_request(&request)?;
            let expected_count = request.inputs.len();
            let expected_dimension = request.dimension;
            let url = embeddings_url(&self.config.base_url);
            let request_builder = self
                .client
                .post(url)
                .bearer_auth(&self.config.api_key)
                .timeout(self.config.timeout)
                .json(&OpenAiEmbeddingRequest {
                    model: &request.model,
                    input: &request.inputs,
                });
            let response = send_embedding_request(&self.qos, request_builder)
                .await
                .map_err(transport_error)?;
            let status = response.status();
            if !status.is_success() {
                return Err(status_error(status.as_u16(), response.text().await.ok()));
            }
            let payload = response
                .json::<OpenAiEmbeddingResponse>()
                .await
                .map_err(|error| permanent_error("invalid_response_json", error.to_string()))?;

            parse_embedding_response(payload, expected_count, expected_dimension)
        })
    }
}

async fn send_embedding_request(
    qos: &Option<(QosRuntime, QosPolicy)>,
    request: reqwest::RequestBuilder,
) -> Result<QosHttpResponse, EmbeddingTransportError> {
    match qos {
        Some((qos, policy)) => send_request_with_qos(qos, policy, request)
            .await
            .map_err(EmbeddingTransportError::Qos),
        None => request
            .send()
            .await
            .map(QosHttpResponse::unmetered)
            .map_err(EmbeddingTransportError::Reqwest),
    }
}

enum EmbeddingTransportError {
    Reqwest(reqwest::Error),
    Qos(QosHttpClientError),
}

struct EchoEmbeddingProvider {
    config: RemoteEmbeddingConfig,
}

impl EmbeddingProvider for EchoEmbeddingProvider {
    fn embed(&self, request: EmbeddingRequest) -> EmbeddingFuture<'_, Vec<EmbeddingVector>> {
        Box::pin(async move {
            validate_request(&request)?;
            let dimension = usize::try_from(request.dimension).map_err(|_| {
                permanent_error("invalid_dimension", "embedding dimension is too large")
            })?;
            let vectors = request
                .inputs
                .iter()
                .map(|input| deterministic_vector(input, dimension))
                .collect::<Vec<_>>();
            let _ = &self.config;

            Ok(vectors)
        })
    }
}

#[derive(Serialize)]
struct OpenAiEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f64>,
}

fn parse_embedding_response(
    response: OpenAiEmbeddingResponse,
    expected_count: usize,
    expected_dimension: u32,
) -> Result<Vec<EmbeddingVector>, EmbeddingProviderError> {
    if response.data.len() != expected_count {
        return Err(permanent_error(
            "embedding_count_mismatch",
            format!(
                "provider returned {} embeddings for {} inputs",
                response.data.len(),
                expected_count
            ),
        ));
    }
    let expected_dimension = usize::try_from(expected_dimension)
        .map_err(|_| permanent_error("invalid_dimension", "embedding dimension is too large"))?;
    response
        .data
        .into_iter()
        .map(|item| validate_vector(item.embedding, expected_dimension))
        .collect()
}

fn validate_request(request: &EmbeddingRequest) -> Result<(), EmbeddingProviderError> {
    if request.inputs.is_empty() {
        return Err(permanent_error(
            "empty_embedding_batch",
            "embedding request must contain at least one input",
        ));
    }
    if request.model.trim().is_empty() {
        return Err(permanent_error(
            "empty_embedding_model",
            "embedding model must not be blank",
        ));
    }
    if request.dimension == 0 {
        return Err(permanent_error(
            "invalid_dimension",
            "embedding dimension must be greater than zero",
        ));
    }

    Ok(())
}

fn validate_vector(
    values: Vec<f64>,
    expected_dimension: usize,
) -> Result<EmbeddingVector, EmbeddingProviderError> {
    if values.len() != expected_dimension {
        return Err(permanent_error(
            "embedding_dimension_mismatch",
            format!(
                "provider returned dimension {} while {} was configured",
                values.len(),
                expected_dimension
            ),
        ));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(permanent_error(
            "invalid_embedding_value",
            "provider returned a non-finite embedding value",
        ));
    }

    Ok(EmbeddingVector { values })
}

fn embeddings_url(base_url: &str) -> String {
    let base = base_url
        .trim()
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    if base.ends_with("/embeddings") {
        return base.to_owned();
    }
    if final_path_segment(base).is_some_and(is_api_version_segment) {
        return format!("{base}/embeddings");
    }

    format!("{base}/v1/embeddings")
}

fn final_path_segment(url: &str) -> Option<&str> {
    let after_authority = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = after_authority.split_once('/')?.1;
    let path = path.split(['?', '#']).next().unwrap_or(path);

    path.rsplit('/').find(|segment| !segment.is_empty())
}

fn is_api_version_segment(segment: &str) -> bool {
    let Some(digits) = segment
        .strip_prefix('v')
        .or_else(|| segment.strip_prefix('V'))
    else {
        return false;
    };

    !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
}

fn deterministic_vector(input: &str, dimension: usize) -> EmbeddingVector {
    let mut values = vec![0.0; dimension];
    for (index, byte) in input.bytes().enumerate() {
        values[index % dimension] += f64::from(byte) / 255.0;
    }
    let norm = values.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm > 0.0 {
        for value in &mut values {
            *value /= norm;
        }
    }

    EmbeddingVector { values }
}

fn status_error(status_code: u16, body: Option<String>) -> EmbeddingProviderError {
    let body_reports_resource_limit = body
        .as_deref()
        .is_some_and(provider_error_reports_resource_limit);
    let resource_limited = matches!(status_code, 402 | 429)
        || (status_allows_resource_limit_body(status_code) && body_reports_resource_limit);
    let retry = if resource_limited || matches!(status_code, 408 | 500..=599) {
        ProviderRetryClass::Retryable
    } else {
        ProviderRetryClass::Permanent
    };

    EmbeddingProviderError {
        retry,
        status_code: Some(status_code),
        code: status_code_error_code(status_code, resource_limited).to_owned(),
        message: body
            .map(error_body_preview)
            .unwrap_or_else(|| "provider request failed".to_owned()),
    }
}

fn transport_error(error: EmbeddingTransportError) -> EmbeddingProviderError {
    let code = if error.is_timeout() {
        "network_timeout"
    } else {
        "network_error"
    };

    EmbeddingProviderError {
        retry: ProviderRetryClass::Retryable,
        status_code: error.status_code(),
        code: code.to_owned(),
        message: error.to_string(),
    }
}

impl EmbeddingTransportError {
    fn is_timeout(&self) -> bool {
        match self {
            Self::Reqwest(error) => error.is_timeout(),
            Self::Qos(error) => error.is_timeout(),
        }
    }

    fn status_code(&self) -> Option<u16> {
        match self {
            Self::Reqwest(error) => error.status().map(|status| status.as_u16()),
            Self::Qos(_) => None,
        }
    }
}

impl fmt::Display for EmbeddingTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reqwest(error) => error.fmt(formatter),
            Self::Qos(error) => error.fmt(formatter),
        }
    }
}

fn permanent_error(code: &'static str, message: impl Into<String>) -> EmbeddingProviderError {
    EmbeddingProviderError {
        retry: ProviderRetryClass::Permanent,
        status_code: None,
        code: code.to_owned(),
        message: message.into(),
    }
}

fn status_code_error_code(status_code: u16, resource_limited: bool) -> &'static str {
    if resource_limited {
        return "rate_limited";
    }

    match status_code {
        400 => "invalid_request",
        401 | 403 => "auth_invalid",
        404 => "model_or_endpoint_not_found",
        408 => "network_timeout",
        500..=599 => "provider_unavailable",
        _ => "provider_http_error",
    }
}

fn status_allows_resource_limit_body(status_code: u16) -> bool {
    matches!(status_code, 400 | 403 | 409 | 425 | 500..=599)
}

fn provider_error_reports_resource_limit(body: &str) -> bool {
    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(body) {
        return json_strings_report_resource_limit(&payload);
    }

    text_reports_resource_limit(body)
}

fn json_strings_report_resource_limit(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => text_reports_resource_limit(text),
        serde_json::Value::Array(values) => values.iter().any(json_strings_report_resource_limit),
        serde_json::Value::Object(fields) => {
            fields.values().any(json_strings_report_resource_limit)
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            false
        }
    }
}

fn text_reports_resource_limit(text: &str) -> bool {
    let normalized = text
        .chars()
        .map(|character| {
            if character == '_' || character == '-' {
                ' '
            } else {
                character.to_ascii_lowercase()
            }
        })
        .collect::<String>();

    [
        "rate limit",
        "too many request",
        "insufficient quota",
        "quota exceeded",
        "quota exhausted",
        "out of quota",
        "insufficient balance",
        "resource exhausted",
        "no resource package",
        "capacity exceeded",
        "billing limit",
        "payment required",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn error_body_preview(value: String) -> String {
    value.chars().take(PROVIDER_ERROR_MESSAGE_LIMIT).collect()
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
