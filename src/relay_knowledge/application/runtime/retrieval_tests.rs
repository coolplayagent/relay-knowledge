use std::time::Duration;

use super::*;
use crate::{
    domain::RerankMode,
    env::{EnvironmentConfig, PlatformKind, RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL},
    retrieval::{EmbeddingProviderKind, ReadModelBackendMode},
};

#[test]
fn resolves_retrieval_read_model_runtime_from_environment() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "external"),
            ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "external"),
            ("RELAY_KNOWLEDGE_LLM_PROVIDER", "openai_compatible"),
            (
                "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
                "https://embeddings.example/v1",
            ),
            ("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", "secret-key"),
            ("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", "text-embed-3-small"),
            ("RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL", "clip-vit-b32"),
            ("RELAY_KNOWLEDGE_EMBEDDING_DIMENSION", "1536"),
            ("RELAY_KNOWLEDGE_EMBEDDING_BATCH_SIZE", "16"),
            ("RELAY_KNOWLEDGE_EMBEDDING_TIMEOUT_MS", "9000"),
            ("RELAY_KNOWLEDGE_EMBEDDING_MAX_CONCURRENCY", "2"),
            ("RELAY_KNOWLEDGE_RERANK_BACKEND", "external"),
            ("RELAY_KNOWLEDGE_RERANK_MODEL", "bge-reranker-v2"),
            ("RELAY_KNOWLEDGE_RERANK_TIMEOUT_MS", "700"),
            ("RELAY_KNOWLEDGE_RERANK_CANDIDATE_MULTIPLIER", "5"),
            ("RELAY_KNOWLEDGE_RERANK_MAX_CANDIDATES", "80"),
        ],
    )
    .expect("environment should parse");

    let runtime = retrieval_config_from_environment(&environment.retrieval)
        .expect("retrieval runtime should compose");

    assert_eq!(runtime.semantic_mode, ReadModelBackendMode::External);
    assert_eq!(runtime.vector_mode, ReadModelBackendMode::External);
    assert_eq!(runtime.vector_model.name, "text-embed-3-small");
    assert_eq!(runtime.image_model.name, "clip-vit-b32");
    assert_eq!(runtime.vector_model.dimension, 1536);
    let remote = runtime
        .remote_embedding
        .expect("remote embedding config should be present");
    assert_eq!(remote.provider, EmbeddingProviderKind::OpenAiCompatible);
    assert_eq!(remote.redacted_base_url(), "https://embeddings.example");
    assert_eq!(remote.batch_size, 16);
    assert_eq!(remote.timeout, Duration::from_millis(9000));
    assert_eq!(remote.max_concurrency, 2);
    assert_eq!(runtime.rerank.mode, RerankMode::External);
    assert_eq!(runtime.rerank.model.as_deref(), Some("bge-reranker-v2"));
    assert_eq!(runtime.rerank.timeout, Duration::from_millis(700));
    assert_eq!(runtime.rerank.candidate_multiplier, 5);
    assert_eq!(runtime.rerank.max_candidates, 80);
}

#[test]
fn rejects_external_backend_without_remote_model_metadata() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "external"),
            (
                "RELAY_KNOWLEDGE_EMBEDDING_BASE_URL",
                "https://embeddings.example/v1",
            ),
            ("RELAY_KNOWLEDGE_EMBEDDING_API_KEY", "secret-key"),
        ],
    )
    .expect("environment should parse");

    let error = retrieval_config_from_environment(&environment.retrieval)
        .expect_err("external backend should require explicit model metadata");

    assert_eq!(
        error,
        RetrievalRuntimeConfigError::MissingRemoteValue(RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL)
    );
}

#[test]
fn rejects_blank_retrieval_model_overrides() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", "   ")],
    )
    .expect("environment should parse");

    let error = retrieval_config_from_environment(&environment.retrieval)
        .expect_err("blank model name should fail");

    assert_eq!(
        error,
        RetrievalRuntimeConfigError::EmptyModelName(RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL)
    );
}

#[test]
fn rejects_unknown_rerank_backend_mode() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_RERANK_BACKEND", "remote")],
    )
    .expect("environment should parse");

    let error = retrieval_config_from_environment(&environment.retrieval)
        .expect_err("unknown rerank backend should fail");

    assert!(matches!(
        error,
        RetrievalRuntimeConfigError::InvalidRerankBackend(_)
    ));
}
