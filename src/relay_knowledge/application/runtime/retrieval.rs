use std::{error::Error, fmt, time::Duration};

use crate::{
    domain::{RerankMode, RerankModeError},
    env::{
        RELAY_KNOWLEDGE_EMBEDDING_API_KEY, RELAY_KNOWLEDGE_EMBEDDING_BASE_URL,
        RELAY_KNOWLEDGE_EMBEDDING_DIMENSION, RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL,
        RELAY_KNOWLEDGE_RERANK_MODEL, RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL, RetrievalEnvOverrides,
    },
    retrieval::{
        DEFAULT_EMBEDDING_BATCH_SIZE, DEFAULT_EMBEDDING_MAX_CONCURRENCY, DEFAULT_EMBEDDING_TIMEOUT,
        DEFAULT_RERANK_CANDIDATE_MULTIPLIER, DEFAULT_RERANK_MAX_CANDIDATES, DEFAULT_RERANK_TIMEOUT,
        EmbeddingProviderKind, EmbeddingProviderKindError, LOCAL_RERANK_MODEL,
        LOCAL_SEMANTIC_MODEL, LOCAL_VECTOR_DIMENSION, LOCAL_VECTOR_MODEL, ReadModelBackendConfig,
        ReadModelBackendMode, ReadModelBackendModeError, ReadModelMetadata, RemoteEmbeddingConfig,
        RerankConfig,
    },
};

/// Retrieval runtime configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetrievalRuntimeConfigError {
    InvalidBackend(ReadModelBackendModeError),
    InvalidRerankBackend(RerankModeError),
    InvalidProvider(EmbeddingProviderKindError),
    EmptyModelName(&'static str),
    MissingRemoteValue(&'static str),
    InvalidRemoteBaseUrl(String),
    DimensionTooLarge(usize),
}

impl fmt::Display for RetrievalRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBackend(error) => write!(formatter, "{error}"),
            Self::InvalidRerankBackend(error) => write!(formatter, "{error}"),
            Self::InvalidProvider(error) => write!(formatter, "{error}"),
            Self::EmptyModelName(variable) => {
                write!(formatter, "{variable} must not be blank")
            }
            Self::MissingRemoteValue(variable) => {
                write!(
                    formatter,
                    "{variable} is required when a read model backend is external"
                )
            }
            Self::InvalidRemoteBaseUrl(value) => {
                write!(
                    formatter,
                    "embedding base URL '{value}' must use http:// or https://"
                )
            }
            Self::DimensionTooLarge(value) => {
                write!(formatter, "embedding dimension {value} does not fit in u32")
            }
        }
    }
}

impl Error for RetrievalRuntimeConfigError {}

pub(super) fn retrieval_config_from_environment(
    overrides: &RetrievalEnvOverrides,
) -> Result<ReadModelBackendConfig, RetrievalRuntimeConfigError> {
    let semantic_mode = parse_backend_mode(overrides.semantic_backend.as_deref())?;
    let vector_mode = parse_backend_mode(overrides.vector_backend.as_deref())?;
    let remote_required = semantic_mode == ReadModelBackendMode::External
        || vector_mode == ReadModelBackendMode::External;
    require_remote_model_metadata(overrides, remote_required)?;
    let dimension = match overrides.embedding_dimension {
        Some(value) => u32::try_from(value)
            .map_err(|_| RetrievalRuntimeConfigError::DimensionTooLarge(value))?,
        None => LOCAL_VECTOR_DIMENSION,
    };
    let text_model = model_name_override(
        overrides.text_embedding_model.as_deref(),
        RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL,
        LOCAL_VECTOR_MODEL,
    )?;
    let semantic_model = model_name_override(
        overrides.text_embedding_model.as_deref(),
        RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL,
        LOCAL_SEMANTIC_MODEL,
    )?;
    let image_model = model_name_override(
        overrides.image_embedding_model.as_deref(),
        RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL,
        "relay-local-image-hash-v1",
    )?;

    let remote_embedding = remote_embedding_config_from_environment(overrides, remote_required)?;
    let rerank = rerank_config_from_environment(overrides)?;

    Ok(ReadModelBackendConfig {
        semantic_mode,
        vector_mode,
        semantic_model: ReadModelMetadata {
            name: semantic_model,
            dimension,
        },
        vector_model: ReadModelMetadata {
            name: text_model,
            dimension,
        },
        image_model: ReadModelMetadata {
            name: image_model,
            dimension,
        },
        remote_embedding,
        rerank,
    })
}

fn rerank_config_from_environment(
    overrides: &RetrievalEnvOverrides,
) -> Result<RerankConfig, RetrievalRuntimeConfigError> {
    let mode = overrides
        .rerank_backend
        .as_deref()
        .map(RerankMode::parse)
        .transpose()
        .map_err(RetrievalRuntimeConfigError::InvalidRerankBackend)?
        .unwrap_or(RerankMode::Local);
    let model = match mode {
        RerankMode::Disabled => None,
        RerankMode::Local => Some(model_name_override(
            overrides.rerank_model.as_deref(),
            RELAY_KNOWLEDGE_RERANK_MODEL,
            LOCAL_RERANK_MODEL,
        )?),
        RerankMode::External => overrides
            .rerank_model
            .as_deref()
            .map(|model| model_name_override(Some(model), RELAY_KNOWLEDGE_RERANK_MODEL, ""))
            .transpose()?,
    };
    let timeout = overrides
        .rerank_timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_RERANK_TIMEOUT);

    Ok(RerankConfig {
        mode,
        model,
        timeout,
        candidate_multiplier: overrides
            .rerank_candidate_multiplier
            .unwrap_or(DEFAULT_RERANK_CANDIDATE_MULTIPLIER),
        max_candidates: overrides
            .rerank_max_candidates
            .unwrap_or(DEFAULT_RERANK_MAX_CANDIDATES),
    })
}

fn require_remote_model_metadata(
    overrides: &RetrievalEnvOverrides,
    required: bool,
) -> Result<(), RetrievalRuntimeConfigError> {
    if !required {
        return Ok(());
    }
    if overrides.text_embedding_model.is_none() {
        return Err(RetrievalRuntimeConfigError::MissingRemoteValue(
            RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL,
        ));
    }
    if overrides.embedding_dimension.is_none() {
        return Err(RetrievalRuntimeConfigError::MissingRemoteValue(
            RELAY_KNOWLEDGE_EMBEDDING_DIMENSION,
        ));
    }

    Ok(())
}

fn remote_embedding_config_from_environment(
    overrides: &RetrievalEnvOverrides,
    required: bool,
) -> Result<Option<RemoteEmbeddingConfig>, RetrievalRuntimeConfigError> {
    if !required {
        return Ok(None);
    }
    let provider = overrides
        .llm_provider
        .as_deref()
        .map(EmbeddingProviderKind::parse)
        .transpose()
        .map_err(RetrievalRuntimeConfigError::InvalidProvider)?
        .unwrap_or(EmbeddingProviderKind::OpenAiCompatible);
    let base_url = required_remote_value(
        overrides.embedding_base_url.as_deref(),
        RELAY_KNOWLEDGE_EMBEDDING_BASE_URL,
    )?;
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err(RetrievalRuntimeConfigError::InvalidRemoteBaseUrl(base_url));
    }
    let api_key = required_remote_value(
        overrides.embedding_api_key.as_deref(),
        RELAY_KNOWLEDGE_EMBEDDING_API_KEY,
    )?;
    let batch_size = overrides
        .embedding_batch_size
        .unwrap_or(DEFAULT_EMBEDDING_BATCH_SIZE);
    let timeout = overrides
        .embedding_timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_EMBEDDING_TIMEOUT);
    let max_concurrency = overrides
        .embedding_max_concurrency
        .unwrap_or(DEFAULT_EMBEDDING_MAX_CONCURRENCY);

    Ok(Some(RemoteEmbeddingConfig {
        provider,
        base_url,
        api_key,
        batch_size,
        timeout,
        max_concurrency,
    }))
}

fn required_remote_value(
    value: Option<&str>,
    variable: &'static str,
) -> Result<String, RetrievalRuntimeConfigError> {
    match value.map(str::trim) {
        Some(trimmed) if !trimmed.is_empty() => Ok(trimmed.to_owned()),
        _ => Err(RetrievalRuntimeConfigError::MissingRemoteValue(variable)),
    }
}

fn model_name_override(
    value: Option<&str>,
    variable: &'static str,
    default: &'static str,
) -> Result<String, RetrievalRuntimeConfigError> {
    match value {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Err(RetrievalRuntimeConfigError::EmptyModelName(variable))
            } else {
                Ok(trimmed.to_owned())
            }
        }
        None => Ok(default.to_owned()),
    }
}

fn parse_backend_mode(
    value: Option<&str>,
) -> Result<ReadModelBackendMode, RetrievalRuntimeConfigError> {
    value
        .map(ReadModelBackendMode::parse)
        .transpose()
        .map_err(RetrievalRuntimeConfigError::InvalidBackend)
        .map(|mode| mode.unwrap_or(ReadModelBackendMode::Local))
}

#[cfg(test)]
#[path = "retrieval_tests.rs"]
mod tests;
