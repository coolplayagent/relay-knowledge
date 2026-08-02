use std::time::Duration;

use super::*;
use crate::env::{EnvironmentConfig, PlatformKind};

#[test]
fn resolves_mcp_agent_runtime_from_environment() {
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

    let runtime = AgentRuntimeConfig::from_environment(&environment, Duration::from_secs(30))
        .expect("agent runtime should compose");

    assert!(runtime.mcp_streamable_http_enabled);
    assert_eq!(runtime.mcp_endpoint, "/relay-mcp");
    assert_eq!(runtime.mcp_allowed_origins, ["http://localhost:3000"]);
    assert_eq!(runtime.access_policy.allowed_scopes, ["docs", "src"]);
    assert!(runtime.access_policy.allow_unspecified_scope);
    assert_eq!(runtime.access_policy.max_limit, 3);
    assert_eq!(runtime.access_policy.max_context_bytes, 4096);
    assert!(runtime.access_policy.allow_remote_clients);
    assert!(runtime.audit_sink_enabled);
    assert_eq!(runtime.audit_queue_depth, 128);
}

#[test]
fn rejects_invalid_mcp_endpoint() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [("RELAY_KNOWLEDGE_MCP_ENDPOINT", "mcp")],
    )
    .expect("environment should parse");

    let error = AgentRuntimeConfig::from_environment(&environment, Duration::from_secs(30))
        .expect_err("invalid endpoint should fail");

    assert!(matches!(error, AgentRuntimeConfigError::InvalidEndpoint(_)));
}
