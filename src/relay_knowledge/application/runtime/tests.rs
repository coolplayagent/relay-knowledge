use std::{path::PathBuf, time::Duration};

use super::*;
use crate::env::{
    PlatformKind, RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT, RELAY_KNOWLEDGE_STORAGE_TOPOLOGY,
};

#[test]
fn file_index_root_ids_use_canonical_paths_when_available() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be valid")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-runtime-root-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("fixture root should be created");

    let direct = FileIndexRootConfig::new("local-files", root.clone());
    let dotted = FileIndexRootConfig::new("local-files", root.join("."));
    assert_eq!(direct.root_id, dotted.root_id);
    assert_eq!(direct.root_path, dotted.root_path);

    std::fs::remove_dir_all(root).expect("fixture root should be removed");
}

#[test]
fn file_index_root_ids_normalize_nonexistent_trailing_separators() {
    let plain = FileIndexRootConfig::new("local-files", PathBuf::from("/opt/docs"));
    let trailing = FileIndexRootConfig::new("local-files", PathBuf::from("/opt/docs/"));

    assert_eq!(plain.root_id, trailing.root_id);
    assert_eq!(plain.root_path, trailing.root_path);
}

#[test]
fn file_index_roots_from_environment_must_be_absolute() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_FILE_INDEX_ROOTS", "docs;/opt/docs")],
    )
    .expect("environment should parse");

    let error = FileIndexRuntimeConfig::from_environment(&environment)
        .expect_err("relative file index roots should be rejected");

    assert_eq!(
        error,
        FileIndexRuntimeConfigError::RelativeRoot("docs".to_owned())
    );
    assert!(error.to_string().contains("absolute path"));
}

#[test]
fn file_index_roots_accept_windows_drive_and_unc_paths() {
    assert!(is_absolute_file_index_root(
        r"D:\Documents",
        PlatformKind::Windows
    ));
    assert!(is_absolute_file_index_root(
        r"\\server\share\Documents",
        PlatformKind::Windows
    ));
    assert!(!is_absolute_file_index_root(
        r"D:Documents",
        PlatformKind::Windows
    ));
}

#[tokio::test]
async fn resolves_code_index_worker_concurrency_from_environment() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [(RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT, "4")],
    )
    .expect("environment should parse");

    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    assert_eq!(runtime.workers.code_index_max_in_flight, 4);
}

#[tokio::test]
async fn caps_code_index_worker_concurrency_from_environment() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [(RELAY_KNOWLEDGE_CODE_INDEX_MAX_IN_FLIGHT, "99")],
    )
    .expect("environment should parse");

    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    assert_eq!(
        runtime.workers.code_index_max_in_flight,
        WorkerRuntimeConfig::MAX_CODE_INDEX_MAX_IN_FLIGHT
    );
}

#[tokio::test]
async fn resolves_storage_topology_from_environment() {
    let default_environment = storage_topology_test_environment(None);
    let default_runtime = RuntimeConfiguration::from_environment(&default_environment)
        .await
        .expect("runtime should compose");

    assert_eq!(
        default_runtime.storage.topology,
        StorageTopology::SingleSqlite
    );

    let partitioned_environment = storage_topology_test_environment(Some("partitioned_sqlite"));
    let partitioned_runtime = RuntimeConfiguration::from_environment(&partitioned_environment)
        .await
        .expect("runtime should compose");

    assert_eq!(
        partitioned_runtime.storage.topology,
        StorageTopology::PartitionedSqlite
    );
}

#[tokio::test]
async fn rejects_invalid_storage_topology_from_environment() {
    let environment = storage_topology_test_environment(Some("distributed_sqlite"));

    let error = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect_err("invalid storage topology should be rejected");

    assert!(error.to_string().contains("single_sqlite"));
    assert!(error.to_string().contains("partitioned_sqlite"));
}

fn storage_topology_test_environment(topology: Option<&str>) -> EnvironmentConfig {
    let suffix = topology.unwrap_or("default");
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-runtime-storage-{suffix}-{}",
        std::process::id()
    ));
    let mut pairs = vec![(
        "RELAY_KNOWLEDGE_HOME".to_owned(),
        root.display().to_string(),
    )];
    if let Some(topology) = topology {
        pairs.push((
            RELAY_KNOWLEDGE_STORAGE_TOPOLOGY.to_owned(),
            topology.to_owned(),
        ));
    }

    EnvironmentConfig::from_pairs(PlatformKind::current(), pairs).expect("environment should parse")
}

#[tokio::test]
async fn resolves_mcp_agent_runtime_from_environment() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED", "true"),
            ("RELAY_KNOWLEDGE_MCP_ENDPOINT", "/relay-mcp"),
            (
                "RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS",
                "http://localhost:3000",
            ),
            ("RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES", "docs,src"),
            ("RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE", "true"),
            ("RELAY_KNOWLEDGE_MCP_MAX_LIMIT", "3"),
            ("RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES", "4096"),
            ("RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS", "true"),
            ("RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED", "true"),
            ("RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH", "128"),
        ],
    )
    .expect("environment should parse");

    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    assert!(runtime.agent.mcp_streamable_http_enabled);
    assert_eq!(runtime.agent.mcp_endpoint, "/relay-mcp");
    assert_eq!(runtime.agent.mcp_allowed_origins, ["http://localhost:3000"]);
    assert_eq!(runtime.agent.access_policy.allowed_scopes, ["docs", "src"]);
    assert!(runtime.agent.access_policy.allow_unspecified_scope);
    assert_eq!(runtime.agent.access_policy.max_limit, 3);
    assert_eq!(runtime.agent.access_policy.max_context_bytes, 4096);
    assert!(runtime.agent.access_policy.allow_remote_clients);
    assert!(runtime.agent.audit_sink_enabled);
    assert_eq!(runtime.agent.audit_queue_depth, 128);
}

#[tokio::test]
async fn resolves_retrieval_read_model_runtime_from_environment() {
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

    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    assert_eq!(
        runtime.retrieval.semantic_mode,
        ReadModelBackendMode::External
    );
    assert_eq!(
        runtime.retrieval.vector_mode,
        ReadModelBackendMode::External
    );
    assert_eq!(runtime.retrieval.vector_model.name, "text-embed-3-small");
    assert_eq!(runtime.retrieval.image_model.name, "clip-vit-b32");
    assert_eq!(runtime.retrieval.vector_model.dimension, 1536);
    let remote = runtime
        .retrieval
        .remote_embedding
        .expect("remote embedding config should be present");
    assert_eq!(remote.provider, EmbeddingProviderKind::OpenAiCompatible);
    assert_eq!(remote.redacted_base_url(), "https://embeddings.example");
    assert_eq!(remote.batch_size, 16);
    assert_eq!(remote.timeout, Duration::from_millis(9000));
    assert_eq!(remote.max_concurrency, 2);
    assert_eq!(runtime.retrieval.rerank.mode, RerankMode::External);
    assert_eq!(
        runtime.retrieval.rerank.model.as_deref(),
        Some("bge-reranker-v2")
    );
    assert_eq!(runtime.retrieval.rerank.timeout, Duration::from_millis(700));
    assert_eq!(runtime.retrieval.rerank.candidate_multiplier, 5);
    assert_eq!(runtime.retrieval.rerank.max_candidates, 80);
}

#[tokio::test]
async fn rejects_external_backend_without_remote_model_metadata() {
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

    let error = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect_err("external backend should require explicit model metadata");

    assert!(matches!(
        error,
        RuntimeConfigurationError::Retrieval(RetrievalRuntimeConfigError::MissingRemoteValue(
            RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL
        ))
    ));
}

#[tokio::test]
async fn rejects_blank_retrieval_model_overrides() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL", "   ")],
    )
    .expect("environment should parse");

    let error = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect_err("blank model name should fail");

    assert!(matches!(
        error,
        RuntimeConfigurationError::Retrieval(RetrievalRuntimeConfigError::EmptyModelName(
            RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL
        ))
    ));
}

#[tokio::test]
async fn rejects_unknown_rerank_backend_mode() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_RERANK_BACKEND", "remote")],
    )
    .expect("environment should parse");

    let error = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect_err("unknown rerank backend should fail");

    assert!(matches!(
        error,
        RuntimeConfigurationError::Retrieval(RetrievalRuntimeConfigError::InvalidRerankBackend(_))
    ));
}

#[tokio::test]
async fn rejects_invalid_mcp_endpoint() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_MCP_ENDPOINT", "mcp")],
    )
    .expect("environment should parse");

    let error = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect_err("invalid endpoint should fail");

    assert!(matches!(
        error,
        RuntimeConfigurationError::Agent(AgentRuntimeConfigError::InvalidEndpoint(_))
    ));
}

#[tokio::test]
async fn rejects_worker_endpoint_without_http_host() {
    for endpoint in ["https://worker.local", "http://", "http://:8792"] {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [("RELAY_KNOWLEDGE_WORKER_OCR_ENDPOINT", endpoint)],
        )
        .expect("environment should parse");

        let error = RuntimeConfiguration::from_environment(&environment)
            .await
            .expect_err("invalid worker endpoint should fail");

        assert!(matches!(
            error,
            RuntimeConfigurationError::Workers(WorkerRuntimeConfigError::InvalidEndpoint(_))
        ));
    }
}
