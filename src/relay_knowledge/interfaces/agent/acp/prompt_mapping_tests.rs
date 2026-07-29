use crate::{
    api::AgentAccessPolicy,
    application::AgentRuntimeConfig,
    domain::{
        CODEGRAPH_CONTEXT_DEFAULT_LIMIT, CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES,
        CODEGRAPH_CONTEXT_MAX_BYTES, CODEGRAPH_CONTEXT_MIN_BYTES, FreshnessPolicy,
    },
    interfaces::agent::acp::{AcpPromptMeta, AcpRelayKnowledgePrompt},
};

use super::*;

#[test]
fn repository_prompt_maps_to_a_bounded_codegraph_request() {
    let mapped = map_prompt_request(
        &agent_config(),
        prompt(AcpRelayKnowledgePrompt {
            query: Some("retry policy".to_owned()),
            repository: Some("docs".to_owned()),
            path_filters: vec![" src ".to_owned()],
            language_filters: vec![" Rust ".to_owned()],
            freshness: Some("wait-until-fresh".to_owned()),
            exclude_generated: Some(true),
            ..AcpRelayKnowledgePrompt::default()
        }),
    )
    .expect("repository prompt should map");

    assert_eq!(mapped.audit_scope().as_deref(), Some("docs"));
    assert_eq!(mapped.source_scope, None);
    assert_eq!(mapped.limit, CODEGRAPH_CONTEXT_DEFAULT_LIMIT);
    let request = mapped
        .into_codegraph_request()
        .expect("request should validate")
        .expect("repository should create codegraph request");
    assert_eq!(request.repository.repository, "docs");
    assert_eq!(request.repository.ref_selector, "HEAD");
    assert_eq!(request.repository.path_filters, ["src"]);
    assert_eq!(request.repository.language_filters, ["Rust"]);
    assert_eq!(request.freshness_policy, FreshnessPolicy::WaitUntilFresh);
    assert!(request.exclude_generated);
}

#[test]
fn graph_prompt_maps_to_hybrid_retrieval_defaults() {
    let mapped = map_prompt_request(
        &agent_config(),
        prompt(AcpRelayKnowledgePrompt {
            query: Some("graph storage".to_owned()),
            source_scope: Some("docs".to_owned()),
            ..AcpRelayKnowledgePrompt::default()
        }),
    )
    .expect("graph prompt should map");

    assert_eq!(mapped.audit_scope().as_deref(), Some("docs"));
    let request = mapped.into_retrieval_request();
    assert_eq!(request.query, "graph storage");
    assert_eq!(request.source_scope.as_deref(), Some("docs"));
    assert_eq!(request.freshness, FreshnessPolicy::AllowStale);
}

#[test]
fn mapping_rejects_empty_queries_and_unknown_freshness() {
    let empty = map_prompt_request(
        &agent_config(),
        prompt(AcpRelayKnowledgePrompt {
            query: Some("  ".to_owned()),
            source_scope: Some("docs".to_owned()),
            ..AcpRelayKnowledgePrompt::default()
        }),
    )
    .expect_err("empty query should fail");
    let invalid_freshness = map_prompt_request(
        &agent_config(),
        prompt(AcpRelayKnowledgePrompt {
            source_scope: Some("docs".to_owned()),
            freshness: Some("eventually".to_owned()),
            ..AcpRelayKnowledgePrompt::default()
        }),
    )
    .expect_err("unknown freshness should fail");

    assert_eq!(empty.kind, AgentAdapterErrorKind::InvalidArgument);
    assert_eq!(
        invalid_freshness.kind,
        AgentAdapterErrorKind::InvalidArgument
    );
}

#[test]
fn codegraph_context_bytes_enforce_domain_bounds() {
    assert_eq!(
        authorize_context_bytes(None, CODEGRAPH_CONTEXT_MAX_BYTES * 4, true)
            .expect("default codegraph budget should be valid"),
        CODEGRAPH_CONTEXT_DEFAULT_MAX_BYTES
    );
    let too_small = authorize_context_bytes(Some(CODEGRAPH_CONTEXT_MIN_BYTES - 1), 1_000_000, true)
        .expect_err("small codegraph budget should be rejected");
    let too_large = authorize_context_bytes(Some(CODEGRAPH_CONTEXT_MAX_BYTES + 1), 1_000_000, true)
        .expect_err("large codegraph budget should be rejected");
    assert_eq!(too_small.kind, AgentAdapterErrorKind::InvalidArgument);
    assert_eq!(too_large.kind, AgentAdapterErrorKind::LimitExceeded);
}

fn prompt(relay_knowledge: AcpRelayKnowledgePrompt) -> AcpPromptRequest {
    AcpPromptRequest {
        prompt: "fallback query".to_owned(),
        request_id: None,
        meta: Some(AcpPromptMeta {
            relay_knowledge: Some(relay_knowledge),
        }),
    }
}

fn agent_config() -> AgentRuntimeConfig {
    AgentRuntimeConfig {
        mcp_streamable_http_enabled: false,
        mcp_endpoint: "/mcp".to_owned(),
        mcp_allowed_origins: Vec::new(),
        access_policy: AgentAccessPolicy::new(
            vec!["docs".to_owned()],
            false,
            50,
            CODEGRAPH_CONTEXT_MAX_BYTES,
            1_000,
            false,
        )
        .expect("policy should be valid"),
        audit_sink_enabled: false,
        audit_queue_depth: AgentRuntimeConfig::DEFAULT_AUDIT_QUEUE_DEPTH,
    }
}
