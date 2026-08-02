use std::time::Duration;

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{get, post},
};
use serde_json::json;
use tokio::net::TcpListener;

use super::*;
use crate::{
    model_provider::{
        ModelCapabilities, ModelProfileSaveRequest, ModelRequestHeader,
        profile::{
            StoredModelProfile, default_connect_timeout_seconds, default_temperature, default_top_p,
        },
        test_support::{echo_request, test_http_config, test_service},
    },
    retrieval::ReadModelBackendConfig,
};

#[tokio::test]
async fn echo_probe_and_discovery_work_for_named_and_override_profiles() {
    let service = test_service("echo-probe");
    let retrieval = ReadModelBackendConfig::local();
    let http = test_http_config();
    service
        .save_profile("echo", echo_request("echo", true), &retrieval)
        .await
        .expect("echo profile should save");

    let probe = service
        .probe(
            &http,
            &retrieval,
            ModelConnectivityProbeRequest {
                profile_name: Some("echo".to_owned()),
                override_config: None,
                timeout_ms: Some(5),
            },
        )
        .await
        .expect("probe should succeed");
    assert!(probe.ok);
    assert_eq!(probe.provider, ModelProviderKind::Echo);
    assert_eq!(probe.token_usage.unwrap().total_tokens, 6);

    let discovery = service
        .discover(
            &http,
            &retrieval,
            ModelDiscoveryRequest {
                profile_name: Some("echo".to_owned()),
                override_config: Some(echo_request("echo-override", false)),
                timeout_ms: Some(5),
            },
        )
        .await
        .expect("discovery should succeed");
    assert!(discovery.ok);
    assert_eq!(discovery.models, vec!["echo-override"]);
}

#[tokio::test]
async fn override_only_probe_and_discovery_do_not_require_default_profile() {
    let service = test_service("override-only");
    let retrieval = ReadModelBackendConfig::local();
    let http = test_http_config();

    let probe = service
        .probe(
            &http,
            &retrieval,
            ModelConnectivityProbeRequest {
                profile_name: None,
                override_config: Some(echo_request("echo-draft", true)),
                timeout_ms: Some(5),
            },
        )
        .await
        .expect("override probe should not need saved profile");
    assert!(probe.ok);
    assert_eq!(probe.model, "echo-draft");

    let discovery = service
        .discover(
            &http,
            &retrieval,
            ModelDiscoveryRequest {
                profile_name: None,
                override_config: Some(echo_request("echo-draft", true)),
                timeout_ms: Some(5),
            },
        )
        .await
        .expect("override discovery should not need saved profile");
    assert!(discovery.ok);
    assert_eq!(discovery.models, vec!["echo-draft"]);
}

#[tokio::test]
async fn unsupported_enterprise_provider_reports_non_retryable_diagnostics() {
    let service = test_service("unsupported");
    let retrieval = ReadModelBackendConfig::local();
    let http = test_http_config();
    service
        .save_profile("base", echo_request("echo", true), &retrieval)
        .await
        .expect("base profile should save");
    let request = ModelProfileSaveRequest {
        provider: ModelProviderKind::Maas,
        model: "maas-model".to_owned(),
        base_url: None,
        api_key: None,
        clear_api_key: false,
        headers: Vec::new(),
        ssl_verify: None,
        context_window: None,
        max_tokens: None,
        temperature: default_temperature(),
        top_p: default_top_p(),
        connect_timeout_seconds: default_connect_timeout_seconds(),
        capabilities: None,
        fallback_policy_id: None,
        fallback_priority: 0,
        catalog_provider_id: None,
        catalog_provider_name: None,
        catalog_model_name: None,
        is_default: true,
    };

    let probe = service
        .probe(
            &http,
            &retrieval,
            ModelConnectivityProbeRequest {
                profile_name: None,
                override_config: Some(request.clone()),
                timeout_ms: None,
            },
        )
        .await
        .expect("unsupported probe should be reported");
    assert!(!probe.ok);
    assert_eq!(probe.error_code.as_deref(), Some("unsupported_auth_source"));
    assert!(!probe.retryable);

    let discovery = service
        .discover(
            &http,
            &retrieval,
            ModelDiscoveryRequest {
                profile_name: None,
                override_config: Some(request),
                timeout_ms: None,
            },
        )
        .await
        .expect("unsupported discovery should be reported");
    assert_eq!(
        discovery.error_code.as_deref(),
        Some("unsupported_auth_source")
    );
}

#[tokio::test]
async fn sends_provider_probe_and_discovery_requests() {
    let base_url = serve_provider_fixture().await;
    let client = reqwest::Client::new();
    let profile = stored_profile(
        ModelProviderKind::OpenAiCompatible,
        "gpt-fixture",
        &base_url,
        Some("secret"),
    );

    let probe_response = send_probe_request(&client, &profile, Some(Duration::from_secs(1))).await;
    let probe = probe_result_from_http(
        profile.clone(),
        Instant::now(),
        now_millis(),
        probe_response,
    )
    .await;
    assert!(probe.ok);
    assert_eq!(probe.token_usage.unwrap().total_tokens, 9);

    let embedding_profile = stored_profile(
        ModelProviderKind::OpenAiCompatible,
        "text-embedding-3-small",
        &base_url,
        Some("secret"),
    );
    let embedding_response =
        send_probe_request(&client, &embedding_profile, Some(Duration::from_secs(1))).await;
    let embedding_probe = probe_result_from_http(
        embedding_profile,
        Instant::now(),
        now_millis(),
        embedding_response,
    )
    .await;
    assert!(embedding_probe.ok);
    assert_eq!(embedding_probe.token_usage.unwrap().total_tokens, 4);

    let discovery_response =
        send_discovery_request(&client, &profile, Some(Duration::from_secs(1))).await;
    let discovery = discovery_result_from_http(
        profile.clone(),
        Instant::now(),
        now_millis(),
        discovery_response,
    )
    .await;
    assert!(discovery.ok);
    assert_eq!(discovery.models, vec!["gpt-fixture", "named-model"]);
    assert_eq!(discovery.model_entries[0].context_window, Some(128_000));
    assert_eq!(discovery.model_entries[0].output_limit, Some(4096));

    let anthropic = stored_profile(
        ModelProviderKind::Anthropic,
        "claude-fixture",
        &base_url,
        Some("anthropic-secret"),
    );
    let failed_probe_response =
        send_probe_request(&client, &anthropic, Some(Duration::from_secs(1))).await;
    let failed_probe = probe_result_from_http(
        anthropic.clone(),
        Instant::now(),
        now_millis(),
        failed_probe_response,
    )
    .await;
    assert!(!failed_probe.ok);
    assert_eq!(failed_probe.error_code.as_deref(), Some("auth_failed"));
    assert!(!failed_probe.diagnostics.auth_valid);

    let failed_discovery_response =
        send_discovery_request(&client, &anthropic, Some(Duration::from_secs(1))).await;
    let failed_discovery = discovery_result_from_http(
        anthropic,
        Instant::now(),
        now_millis(),
        failed_discovery_response,
    )
    .await;
    assert!(!failed_discovery.ok);
    assert_eq!(failed_discovery.error_code.as_deref(), Some("rate_limited"));
    assert!(failed_discovery.retryable);

    let invalid_json = stored_profile(
        ModelProviderKind::OpenAiCompatible,
        "gpt-fixture",
        &format!("{base_url}/bad"),
        Some("secret"),
    );
    let invalid_discovery_response =
        send_discovery_request(&client, &invalid_json, Some(Duration::from_secs(1))).await;
    let invalid_discovery = discovery_result_from_http(
        invalid_json,
        Instant::now(),
        now_millis(),
        invalid_discovery_response,
    )
    .await;
    assert!(!invalid_discovery.ok);
    assert_eq!(
        invalid_discovery.error_code.as_deref(),
        Some("invalid_response")
    );
}

#[tokio::test]
async fn transport_failures_return_retryable_diagnostics() {
    let client = reqwest::Client::new();
    let profile = stored_profile(
        ModelProviderKind::OpenAiCompatible,
        "gpt-fixture",
        "http://127.0.0.1:1",
        Some("secret"),
    );
    let response = send_probe_request(&client, &profile, Some(Duration::from_millis(10))).await;

    let result = probe_result_from_http(profile, Instant::now(), now_millis(), response).await;

    assert!(!result.ok);
    assert!(matches!(
        result.error_code.as_deref(),
        Some("network_error" | "network_timeout")
    ));
    assert!(result.retryable);
    assert!(!result.diagnostics.endpoint_reachable);
}

#[test]
fn maps_headers_statuses_and_discovery_payloads() {
    let openai = stored_profile(
        ModelProviderKind::OpenAiCompatible,
        "gpt-fixture",
        "https://api.example.com/v1",
        Some("secret"),
    );
    let openai_headers = auth_headers(&openai);
    assert_eq!(
        openai_headers
            .get("authorization")
            .expect("authorization")
            .to_str()
            .unwrap(),
        "Bearer secret"
    );
    assert_eq!(
        openai_headers
            .get("x-custom")
            .expect("custom header")
            .to_str()
            .unwrap(),
        "custom"
    );

    let anthropic = stored_profile(
        ModelProviderKind::Anthropic,
        "claude-fixture",
        "https://api.anthropic.com",
        Some("secret"),
    );
    let anthropic_headers = auth_headers(&anthropic);
    assert_eq!(
        anthropic_headers
            .get("x-api-key")
            .expect("x-api-key")
            .to_str()
            .unwrap(),
        "secret"
    );
    assert!(anthropic_headers.get("anthropic-version").is_some());

    assert_eq!(status_error_code(401), "auth_failed");
    assert_eq!(status_error_code(408), "network_timeout");
    assert_eq!(status_error_code(429), "rate_limited");
    assert_eq!(status_error_code(503), "provider_error");
    assert_eq!(status_error_code(418), "http_error");
    assert!(is_retryable_status(500));
    assert!(!is_retryable_status(404));
    assert!(diagnostics_from_status(429).rate_limited);
    assert!(ok_diagnostics().auth_valid);

    let usage = token_usage(&json!({
        "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
    }))
    .expect("usage should parse");
    assert_eq!(usage.total_tokens, 3);
    assert!(token_usage(&json!({})).is_none());

    let entries = parse_discovery_entries(&json!({
        "data": [
            {"id": "id-model", "context_window": 2000, "output_limit": 500},
            {"name": "name-model"},
            {"missing": true}
        ]
    }));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].model, "id-model");
    assert_eq!(entries[1].model, "name-model");

    assert_eq!(
        redacted_url("https://user:pass@example.com/v1"),
        "https://example.com/v1"
    );
    assert_eq!(
        redacted_url("https://example.com/tenant@region/v1"),
        "https://example.com/tenant@region/v1"
    );
    assert_eq!(
        redacted_url("https://example.com/v1?tenant=user@example.com"),
        "https://example.com/v1?tenant=user@example.com"
    );
    assert_eq!(
        request_timeout_from_ms(Some(5)),
        Some(Duration::from_millis(5))
    );
    assert_eq!(request_timeout_from_ms(None), None);
}

async fn serve_provider_fixture() -> String {
    let app = Router::new()
        .route(
            "/chat/completions",
            post(|| async {
                Json(json!({
                    "usage": {
                        "prompt_tokens": 3,
                        "completion_tokens": 6,
                        "total_tokens": 9
                    }
                }))
            }),
        )
        .route(
            "/models",
            get(|| async {
                Json(json!({
                    "data": [
                        {"id": "gpt-fixture", "context_window": 128000, "output_limit": 4096},
                        {"name": "named-model"}
                    ]
                }))
            }),
        )
        .route(
            "/embeddings",
            post(|| async {
                Json(json!({
                    "data": [{"embedding": [0.1, 0.2]}],
                    "usage": {
                        "prompt_tokens": 4,
                        "total_tokens": 4
                    }
                }))
            }),
        )
        .route("/bad/models", get(|| async { "not-json" }))
        .route(
            "/v1/messages",
            post(|| async { (StatusCode::UNAUTHORIZED, Json(json!({}))) }),
        )
        .route(
            "/v1/models",
            get(|| async { (StatusCode::TOO_MANY_REQUESTS, Json(json!({}))) }),
        );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("fixture server should run");
    });
    format!("http://{address}")
}

fn stored_profile(
    provider: ModelProviderKind,
    model: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> StoredModelProfile {
    StoredModelProfile {
        provider,
        model: model.to_owned(),
        base_url: base_url.to_owned(),
        api_key: api_key.map(ToOwned::to_owned),
        headers: vec![
            ModelRequestHeader {
                name: "x-custom".to_owned(),
                value: Some("custom".to_owned()),
                secret: true,
                configured: false,
            },
            ModelRequestHeader {
                name: "bad header".to_owned(),
                value: Some("ignored".to_owned()),
                secret: true,
                configured: false,
            },
        ],
        ssl_verify: None,
        context_window: None,
        max_tokens: Some(16),
        temperature: default_temperature(),
        top_p: default_top_p(),
        connect_timeout_seconds: default_connect_timeout_seconds(),
        capabilities: ModelCapabilities::default(),
        fallback_policy_id: None,
        fallback_priority: 0,
        catalog_provider_id: None,
        catalog_provider_name: None,
        catalog_model_name: None,
        is_default: false,
        source: "test".to_owned(),
    }
}
