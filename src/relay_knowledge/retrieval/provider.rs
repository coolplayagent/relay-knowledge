use std::{error::Error, fmt, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};

use super::{EmbeddingProviderKind, RemoteEmbeddingConfig};

const PROVIDER_ERROR_MESSAGE_LIMIT: usize = 240;

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
                .timeout(self.config.timeout)
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
        let provider_error = payload.get("error").unwrap_or(&payload);
        if let Some(text) = provider_error.as_str() {
            return text_reports_resource_limit(text);
        }

        let mut inspected_fields = false;
        for field in ["code", "type", "message", "status", "detail"] {
            if let Some(text) = provider_error
                .get(field)
                .and_then(serde_json::Value::as_str)
            {
                inspected_fields = true;
                if text_reports_resource_limit(text) {
                    return true;
                }
            }
        }
        if inspected_fields {
            return false;
        }
    }

    text_reports_resource_limit(body)
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
        assert_eq!(
            embeddings_url("https://example.test/v4"),
            "https://example.test/v4/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/openai/v2"),
            "https://example.test/openai/v2/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/openai"),
            "https://example.test/openai/v1/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/openai/v4/?probe=true#fragment"),
            "https://example.test/openai/v4/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/v4/embeddings?probe=true"),
            "https://example.test/v4/embeddings"
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

    #[test]
    fn classifies_provider_resource_limit_bodies_as_retryable() {
        let payment_required = status_error(402, None);
        let quota_forbidden = status_error(
            403,
            Some(
                r#"{"error":{"code":"insufficient_quota","message":"Insufficient balance or no resource package."}}"#
                    .to_owned(),
            ),
        );
        let invalid_request_quota = status_error(
            400,
            Some(
                r#"{"error":{"type":"resource_exhausted","message":"quota exceeded"}}"#.to_owned(),
            ),
        );
        let retry_after_resource_exhausted = status_error(
            503,
            Some(
                r#"{"error":{"status":"RESOURCE_EXHAUSTED","message":"rate limit exceeded"}}"#
                    .to_owned(),
            ),
        );

        assert_eq!(payment_required.retry, ProviderRetryClass::Retryable);
        assert_eq!(payment_required.code, "rate_limited");
        assert_eq!(quota_forbidden.retry, ProviderRetryClass::Retryable);
        assert_eq!(quota_forbidden.code, "rate_limited");
        assert_eq!(invalid_request_quota.retry, ProviderRetryClass::Retryable);
        assert_eq!(invalid_request_quota.code, "rate_limited");
        assert_eq!(
            retry_after_resource_exhausted.retry,
            ProviderRetryClass::Retryable
        );
        assert_eq!(retry_after_resource_exhausted.code, "rate_limited");
    }

    #[test]
    fn preserves_permanent_provider_errors_without_resource_limit_signals() {
        let auth_forbidden = status_error(
            403,
            Some(r#"{"error":{"code":"invalid_api_key","message":"Invalid API key"}}"#.to_owned()),
        );
        let invalid_request = status_error(
            400,
            Some(
                r#"{"error":{"code":"invalid_request","message":"quota field is not supported"}}"#
                    .to_owned(),
            ),
        );

        assert_eq!(auth_forbidden.retry, ProviderRetryClass::Permanent);
        assert_eq!(auth_forbidden.code, "auth_invalid");
        assert_eq!(invalid_request.retry, ProviderRetryClass::Permanent);
        assert_eq!(invalid_request.code, "invalid_request");
    }

    #[test]
    fn classifies_provider_http_status_codes() {
        for (status, code, retry) in [
            (400, "invalid_request", ProviderRetryClass::Permanent),
            (401, "auth_invalid", ProviderRetryClass::Permanent),
            (403, "auth_invalid", ProviderRetryClass::Permanent),
            (
                404,
                "model_or_endpoint_not_found",
                ProviderRetryClass::Permanent,
            ),
            (408, "network_timeout", ProviderRetryClass::Retryable),
            (500, "provider_unavailable", ProviderRetryClass::Retryable),
            (418, "provider_http_error", ProviderRetryClass::Permanent),
        ] {
            let error = status_error(status, Some("x".repeat(300)));

            assert_eq!(error.code, code);
            assert_eq!(error.retry, retry);
            assert_eq!(error.message.len(), 240);
        }
    }

    #[tokio::test]
    async fn openai_provider_posts_and_parses_embeddings() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("local addr should load");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("request should connect");
            let mut buffer = vec![0; 2048];
            let count = stream
                .readable()
                .await
                .and_then(|()| stream.try_read(&mut buffer));
            let request = String::from_utf8_lossy(&buffer[..count.expect("request should read")]);

            assert!(request.starts_with("POST /v1/embeddings HTTP/1.1"));
            assert!(request.contains("authorization: Bearer secret"));
            assert!(request.contains("\"model\":\"text-embedding-3-small\""));
            stream
                .writable()
                .await
                .expect("stream should become writable");
            stream
                .try_write(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 34\r\n\r\n{\"data\":[{\"embedding\":[0.1,0.2]}]}",
                )
                .expect("response should write");
        });
        let provider = OpenAiCompatibleEmbeddingProvider {
            config: remote_config(
                format!("http://{addr}/v1"),
                std::time::Duration::from_secs(5),
            ),
            client: reqwest::Client::new(),
        };

        let vectors = provider
            .embed(EmbeddingRequest {
                inputs: vec!["probe".to_owned()],
                model: "text-embedding-3-small".to_owned(),
                dimension: 2,
            })
            .await
            .expect("provider response should parse");

        assert_eq!(vectors[0].values, [0.1, 0.2]);
        server.await.expect("server should finish");
    }

    #[tokio::test]
    async fn echo_provider_returns_deterministic_vectors() {
        let provider = EchoEmbeddingProvider {
            config: remote_config("http://example.test/v1", std::time::Duration::from_secs(5)),
        };

        let vectors = provider
            .embed(EmbeddingRequest {
                inputs: vec!["abc".to_owned(), "abc".to_owned()],
                model: "echo".to_owned(),
                dimension: 4,
            })
            .await
            .expect("echo provider should embed");

        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0], vectors[1]);
        assert_eq!(vectors[0].values.len(), 4);
    }

    #[test]
    fn rejects_invalid_requests_and_response_values() {
        let empty = validate_request(&EmbeddingRequest {
            inputs: Vec::new(),
            model: "model".to_owned(),
            dimension: 1,
        })
        .expect_err("empty inputs should fail");
        let model = validate_request(&EmbeddingRequest {
            inputs: vec!["x".to_owned()],
            model: " ".to_owned(),
            dimension: 1,
        })
        .expect_err("blank model should fail");
        let dimension = validate_request(&EmbeddingRequest {
            inputs: vec!["x".to_owned()],
            model: "model".to_owned(),
            dimension: 0,
        })
        .expect_err("zero dimension should fail");
        let invalid_value = validate_vector(vec![f64::NAN], 1).expect_err("nan values should fail");

        assert_eq!(empty.code, "empty_embedding_batch");
        assert_eq!(model.code, "empty_embedding_model");
        assert_eq!(dimension.code, "invalid_dimension");
        assert_eq!(invalid_value.code, "invalid_embedding_value");
    }

    #[tokio::test]
    async fn applies_configured_embedding_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("local addr should load");
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("request should connect");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        });
        let provider = OpenAiCompatibleEmbeddingProvider {
            config: RemoteEmbeddingConfig {
                provider: EmbeddingProviderKind::OpenAiCompatible,
                base_url: format!("http://{addr}/v1"),
                api_key: "secret".to_owned(),
                batch_size: 1,
                timeout: std::time::Duration::from_millis(20),
                max_concurrency: 1,
            },
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("client should build"),
        };

        let error = provider
            .embed(EmbeddingRequest {
                inputs: vec!["probe".to_owned()],
                model: "text-embedding-3-small".to_owned(),
                dimension: 3,
            })
            .await
            .expect_err("provider request should use embedding timeout");

        assert_eq!(error.code, "network_timeout");
        server.abort();
    }

    fn remote_config(
        base_url: impl Into<String>,
        timeout: std::time::Duration,
    ) -> RemoteEmbeddingConfig {
        RemoteEmbeddingConfig {
            provider: EmbeddingProviderKind::OpenAiCompatible,
            base_url: base_url.into(),
            api_key: "secret".to_owned(),
            batch_size: 1,
            timeout,
            max_concurrency: 1,
        }
    }
}
