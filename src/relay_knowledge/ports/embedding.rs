use std::{error::Error, fmt, future::Future, pin::Pin};

/// Future returned by a configured embedding provider.
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

#[cfg(test)]
#[path = "embedding_tests.rs"]
mod tests;
