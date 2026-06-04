use std::path::PathBuf;

use super::*;

#[test]
fn parses_platform_and_relay_overrides() {
    let config = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            (HOME, "/home/alice"),
            (TMPDIR, "/var/tmp"),
            (RELAY_KNOWLEDGE_HOME, "/opt/relay-runtime"),
            (RELAY_KNOWLEDGE_HTTP_BIND, "127.0.0.1:9000"),
            (RELAY_KNOWLEDGE_REMOTE_BASE_URL, "http://relay.example:8791"),
            (HTTPS_PROXY, "https://proxy.internal:8443"),
            (NO_PROXY, "localhost,.internal"),
            (SSL_VERIFY, "false"),
            (RELAY_KNOWLEDGE_QOS_MAX_CONNECTIONS, "512"),
            (RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED, "true"),
            (RELAY_KNOWLEDGE_MCP_ENDPOINT, "/mcp"),
            (RELAY_KNOWLEDGE_MCP_ALLOWED_ORIGINS, "http://localhost:8791"),
            (RELAY_KNOWLEDGE_MCP_ALLOWED_SCOPES, "docs,src"),
            (RELAY_KNOWLEDGE_MCP_ALLOW_UNSPECIFIED_SCOPE, "true"),
            (RELAY_KNOWLEDGE_MCP_MAX_LIMIT, "5"),
            (RELAY_KNOWLEDGE_MCP_MAX_CONTEXT_BYTES, "8192"),
            (RELAY_KNOWLEDGE_MCP_ALLOW_REMOTE_CLIENTS, "false"),
            (RELAY_KNOWLEDGE_AGENT_AUDIT_SINK_ENABLED, "true"),
            (RELAY_KNOWLEDGE_AGENT_AUDIT_QUEUE_DEPTH, "256"),
            (RELAY_KNOWLEDGE_SEMANTIC_BACKEND, "external"),
            (RELAY_KNOWLEDGE_VECTOR_BACKEND, "external"),
            (RELAY_KNOWLEDGE_TEXT_EMBEDDING_MODEL, "text-embed-3-small"),
            (RELAY_KNOWLEDGE_IMAGE_EMBEDDING_MODEL, "clip-vit-b32"),
            (RELAY_KNOWLEDGE_EMBEDDING_DIMENSION, "1536"),
            (RELAY_KNOWLEDGE_FILE_INDEX_SCAN_TIMEOUT_MS, "120000"),
            (RELAY_KNOWLEDGE_FILE_QUERY_TIMEOUT_MS, "600"),
            (RELAY_KNOWLEDGE_UPDATE_CHECK_ENABLED, "true"),
            (RELAY_KNOWLEDGE_UPDATE_SOURCES, "github,crates.io"),
            (RELAY_KNOWLEDGE_UPDATE_CHECK_INTERVAL_MS, "86400000"),
            (
                RELAY_KNOWLEDGE_UPDATE_GITHUB_REPO,
                "coolplayagent/relay-knowledge",
            ),
            (RELAY_OTEL_ENDPOINT, "http://collector.internal:4318"),
            (RELAY_OTEL_TRACES, "true"),
            (RELAY_OTEL_METRICS, "false"),
            (RELAY_OTEL_EXPORT_TIMEOUT_MS, "1500"),
            (RELAY_OTEL_SERVICE_ENVIRONMENT, "test"),
        ],
    )
    .expect("environment should parse");

    assert_eq!(config.platform.platform, PlatformKind::Unix);
    assert_eq!(config.platform.home_dir, Some(PathBuf::from("/home/alice")));
    assert_eq!(config.platform.temp_dir, Some(PathBuf::from("/var/tmp")));
    assert_eq!(config.paths.home, Some(PathBuf::from("/opt/relay-runtime")));
    assert_eq!(config.network.http_bind, Some("127.0.0.1:9000".to_owned()));
    assert_eq!(
        config.remote_cli.base_url,
        Some("http://relay.example:8791".to_owned())
    );
    assert_eq!(
        config.network.proxy,
        Some("https://proxy.internal:8443".to_owned())
    );
    assert_eq!(
        config.network.no_proxy,
        Some("localhost,.internal".to_owned())
    );
    assert_eq!(config.network.ssl_verify, Some(false));
    assert_eq!(config.network.qos_max_connections, Some(512));
    assert_eq!(config.agent.mcp_streamable_http_enabled, Some(true));
    assert_eq!(config.agent.mcp_endpoint, Some("/mcp".to_owned()));
    assert_eq!(
        config.agent.mcp_allowed_origins,
        Some("http://localhost:8791".to_owned())
    );
    assert_eq!(config.agent.mcp_allowed_scopes, Some("docs,src".to_owned()));
    assert_eq!(config.agent.mcp_allow_unspecified_scope, Some(true));
    assert_eq!(config.agent.mcp_max_limit, Some(5));
    assert_eq!(config.agent.mcp_max_context_bytes, Some(8192));
    assert_eq!(config.agent.mcp_allow_remote_clients, Some(false));
    assert_eq!(config.agent.audit_sink_enabled, Some(true));
    assert_eq!(config.agent.audit_queue_depth, Some(256));
    assert_eq!(
        config.retrieval.semantic_backend,
        Some("external".to_owned())
    );
    assert_eq!(config.retrieval.vector_backend, Some("external".to_owned()));
    assert_eq!(
        config.retrieval.text_embedding_model,
        Some("text-embed-3-small".to_owned())
    );
    assert_eq!(
        config.retrieval.image_embedding_model,
        Some("clip-vit-b32".to_owned())
    );
    assert_eq!(config.retrieval.embedding_dimension, Some(1536));
    assert_eq!(config.file_index.scan_timeout_ms, Some(120000));
    assert_eq!(config.file_index.query_timeout_ms, Some(600));
    assert_eq!(config.updates.enabled, Some(true));
    assert_eq!(config.updates.sources, Some("github,crates.io".to_owned()));
    assert_eq!(config.updates.check_interval_ms, Some(86_400_000));
    assert_eq!(
        config.updates.github_repo,
        Some("coolplayagent/relay-knowledge".to_owned())
    );
    assert_eq!(
        config.telemetry.otel_endpoint,
        Some("http://collector.internal:4318".to_owned())
    );
    assert_eq!(config.telemetry.otel_traces, Some(true));
    assert_eq!(config.telemetry.otel_metrics, Some(false));
    assert_eq!(config.telemetry.export_timeout_ms, Some(1500));
    assert_eq!(
        config.telemetry.service_environment,
        Some("test".to_owned())
    );
}
