use std::{error::Error, fmt, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};

use super::{EmbeddingProviderKind, RemoteEmbeddingConfig};

pub type EmbeddingFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, EmbeddingProviderError>> + Send + 'a>>;

/// Text inputs sent to a remote embedding provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRequest {
    pub inputs: Vec<String>,
    pub model: String,
    pub dimension: u32,
}

/// One normalized embedding vector returned by a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingVector {
    pub values: Vec<f64>,
}

/// Provider-neutral remote embedding contract.
pub trait EmbeddingProvider: Send + Sync {
    fn embed(&self, request: EmbeddingRequest) -> EmbeddingFuture<'_, Vec<EmbeddingVector>>;
}

/// Retry category for remote provider failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRetryClass {
    Retryable,
    Permanent,
}

/// Provider error safe for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProviderError {
    pub retry: ProviderRetryClass,
    pub status_code: Option<u16>,
    pub code: String,
    pub message: String,
}

impl fmt::Display for EmbeddingProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.status_code {
            Some(status) => write!(formatter, "{} ({status}): {}", self.code, self.message),
            None => write!(formatter, "{}: {}", self.code, self.message),
        }
    }
}

impl Error for EmbeddingProviderError {}

/// Builds the configured remote embedding provider.
pub fn embedding_provider(
    config: RemoteEmbeddingConfig,
    client: reqwest::Client,
) -> Box<dyn EmbeddingProvider> {
    match config.provider {
        EmbeddingProviderKind::OpenAiCompatible => {
            Box::new(OpenAiCompatibleEmbeddingProvider { config, client })
        }
        EmbeddingProviderKind::Echo => Box::new(EchoEmbeddingProvider { config }),
    }
}

struct OpenAiCompatibleEmbeddingProvider {
    config: RemoteEmbeddingConfig,
    client: reqwest::Client,
}

impl EmbeddingProvider for OpenAiCompatibleEmbeddingProvider {
    fn embed(&self, request: EmbeddingRequest) -> EmbeddingFuture<'_, Vec<EmbeddingVector>> {
        Box::pin(async move {
            validate_request(&request)?;
            let url = embeddings_url(&self.config.base_url);
            let response = self
                .client
                .post(url)
                .bearer_auth(&self.config.api_key)
                .json(&OpenAiEmbeddingRequest {
                    model: &request.model,
                    input: &request.inputs,
                })
                .send()
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

            parse_embedding_response(payload, request.inputs.len(), request.dimension)
        })
    }
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
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/embeddings") {
        return trimmed.to_owned();
    }
    if trimmed.ends_with("/v1") {
        return format!("{trimmed}/embeddings");
    }

    format!("{trimmed}/v1/embeddings")
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
    let retry = if matches!(status_code, 408 | 429 | 500..=599) {
        ProviderRetryClass::Retryable
    } else {
        ProviderRetryClass::Permanent
    };

    EmbeddingProviderError {
        retry,
        status_code: Some(status_code),
        code: status_code_error_code(status_code).to_owned(),
        message: body
            .map(|value| value.chars().take(240).collect())
            .unwrap_or_else(|| "provider request failed".to_owned()),
    }
}

fn transport_error(error: reqwest::Error) -> EmbeddingProviderError {
    let code = if error.is_timeout() {
        "network_timeout"
    } else {
        "network_error"
    };

    EmbeddingProviderError {
        retry: ProviderRetryClass::Retryable,
        status_code: error.status().map(|status| status.as_u16()),
        code: code.to_owned(),
        message: error.to_string(),
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

fn status_code_error_code(status_code: u16) -> &'static str {
    match status_code {
        400 => "invalid_request",
        401 | 403 => "auth_invalid",
        404 => "model_or_endpoint_not_found",
        408 => "network_timeout",
        429 => "rate_limited",
        500..=599 => "provider_unavailable",
        _ => "provider_http_error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeddings_url_accepts_base_or_endpoint() {
        assert_eq!(
            embeddings_url("https://example.test"),
            "https://example.test/v1/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/v1"),
            "https://example.test/v1/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/v1/embeddings"),
            "https://example.test/v1/embeddings"
        );
    }

    #[test]
    fn rejects_embedding_dimension_mismatch() {
        let response = OpenAiEmbeddingResponse {
            data: vec![OpenAiEmbeddingData {
                embedding: vec![0.1, 0.2],
            }],
        };

        let error = parse_embedding_response(response, 1, 3).expect_err("dimension should fail");

        assert_eq!(error.code, "embedding_dimension_mismatch");
        assert_eq!(error.retry, ProviderRetryClass::Permanent);
    }

    #[test]
    fn classifies_rate_limit_as_retryable() {
        let error = status_error(429, None);

        assert_eq!(error.retry, ProviderRetryClass::Retryable);
        assert_eq!(error.code, "rate_limited");
    }
}
