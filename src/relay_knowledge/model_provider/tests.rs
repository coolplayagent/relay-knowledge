use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{
    net::http::{HttpBindAddress, HttpConfig, HttpProxyConfig},
    retrieval::ReadModelBackendConfig,
};
use tokio::io::AsyncWriteExt;

use super::*;

#[tokio::test]
async fn profile_crud_preserves_secrets_and_redacts_responses() {
    let service = test_service("crud");
    let retrieval = ReadModelBackendConfig::local();
    let saved = service
        .save_profile(
            " primary ",
            openai_request("gpt-a", Some("secret-a")),
            &retrieval,
        )
        .await
        .expect("profile should save");

    assert_eq!(saved.default_profile.as_deref(), Some("primary"));
    assert_eq!(saved.profiles[0].base_url, "https://api.example.com/v1");
    assert!(saved.profiles[0].api_key_configured);
    assert_eq!(saved.profiles[0].headers[0].value, None);
    assert!(saved.profiles[0].headers[0].configured);

    let updated = service
        .save_profile("primary", openai_request("gpt-b", None), &retrieval)
        .await
        .expect("profile should update");
    assert_eq!(updated.profiles[0].model, "gpt-b");
    assert!(updated.profiles[0].api_key_configured);

    let raw = fs::read_to_string(service.paths.model_profiles_file())
        .await
        .expect("profile file should exist");
    assert!(raw.contains("secret-a"));
    assert!(raw.contains("header-secret"));
    assert!(
        !serde_json::to_string(&updated)
            .unwrap()
            .contains("secret-a")
    );

    let mut redacted_header_update = openai_request("gpt-c", None);
    redacted_header_update.headers = vec![ModelRequestHeader {
        name: "x-extra-secret".to_owned(),
        value: None,
        secret: true,
        configured: true,
    }];
    service
        .save_profile("primary", redacted_header_update, &retrieval)
        .await
        .expect("redacted header update should preserve stored header value");
    let raw = fs::read_to_string(service.paths.model_profiles_file())
        .await
        .expect("profile file should exist");
    assert!(raw.contains("header-secret"));

    let mut clear_key_update = openai_request("gpt-d", None);
    clear_key_update.clear_api_key = true;
    clear_key_update.headers = vec![ModelRequestHeader {
        name: "x-extra-secret".to_owned(),
        value: Some("header-secret".to_owned()),
        secret: true,
        configured: false,
    }];
    let cleared = service
        .save_profile("primary", clear_key_update, &retrieval)
        .await
        .expect("header-auth update should clear stored api key");
    assert!(!cleared.profiles[0].api_key_configured);
    let raw = fs::read_to_string(service.paths.model_profiles_file())
        .await
        .expect("profile file should exist");
    assert!(!raw.contains("secret-a"));
}

#[tokio::test]
async fn delete_profile_reassigns_default_and_reports_summary() {
    let service = test_service("delete");
    let retrieval = ReadModelBackendConfig::local();
    service
        .save_profile("first", echo_request("echo-a", true), &retrieval)
        .await
        .expect("first profile should save");
    service
        .save_profile("second", echo_request("echo-b", false), &retrieval)
        .await
        .expect("second profile should save");

    let response = service
        .delete_profile("first", &retrieval)
        .await
        .expect("profile should delete");
    assert_eq!(response.default_profile.as_deref(), Some("second"));
    assert_eq!(response.profiles.len(), 1);
    assert!(response.profiles[0].is_default);

    let summary = service.profile_summary(&retrieval).await;
    assert!(summary.loaded);
    assert_eq!(summary.profile_count, 1);
    assert_eq!(summary.default_profile.as_deref(), Some("second"));
}

#[tokio::test]
async fn fallback_config_round_trips_and_rejects_invalid_values() {
    let service = test_service("fallback");
    let default = service
        .fallback_config()
        .await
        .expect("default fallback should load");
    assert_eq!(default.policies.len(), 2);

    let mut custom = default.clone();
    custom.policies[0].policy_id = "fast".to_owned();
    custom.policies[0].max_hops = 2;
    let saved = service
        .save_fallback_config(custom.clone())
        .await
        .expect("fallback should save");
    assert_eq!(saved, custom);
    assert_eq!(service.fallback_config().await.unwrap(), custom);

    custom.policies[0].cooldown_seconds = 3601;
    assert!(service.save_fallback_config(custom).await.is_err());
}

#[tokio::test]
async fn catalog_uses_builtin_cache_and_network_fallbacks() {
    let mut service = test_service("catalog");
    let http = test_http_config();
    let builtin = service
        .catalog(&http, false)
        .await
        .expect("builtin catalog");
    assert!(builtin.ok);
    assert!(
        builtin
            .providers
            .iter()
            .any(|provider| provider.id == "echo")
    );

    let cache = ModelCatalogCache {
        source_url: "fixture".to_owned(),
        fetched_at_ms: now_millis(),
        providers: vec![catalog_provider(
            "fixture",
            "Fixture",
            ModelProviderKind::Echo,
            &["echo-fixture"],
        )],
    };
    service
        .write_catalog_cache(&cache)
        .await
        .expect("cache should write");
    let cached = service.catalog(&http, false).await.expect("cached catalog");
    assert_eq!(cached.source_url, "fixture");
    assert_eq!(cached.providers[0].models[0].id, "echo-fixture");

    service.catalog_source_url = "http://127.0.0.1:1/models".to_owned();
    let fallback = service
        .catalog(&http, true)
        .await
        .expect("fallback catalog");
    assert!(!fallback.ok);
    assert!(fallback.stale);
    assert_eq!(fallback.providers[0].id, "fixture");
    assert_eq!(fallback.error_code.as_deref(), Some("network_error"));

    let mut service = test_service("catalog-http-failure");
    service.catalog_source_url = failing_catalog_url().await;
    let fallback = service
        .catalog(&http, true)
        .await
        .expect("builtin fallback");
    assert!(!fallback.ok);
    assert!(fallback.stale);
    assert!(
        fallback
            .providers
            .iter()
            .any(|provider| provider.id == "echo")
    );
    assert_eq!(fallback.error_code.as_deref(), Some("provider_error"));
}

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
async fn profile_validation_rejects_invalid_inputs() {
    let service = test_service("validation");
    let retrieval = ReadModelBackendConfig::local();

    assert!(
        service
            .save_profile("bad name", echo_request("echo", true), &retrieval)
            .await
            .is_err()
    );
    let mut missing_auth = openai_request("gpt", None);
    missing_auth.headers.clear();
    assert!(
        service
            .save_profile("missing-auth", missing_auth, &retrieval)
            .await
            .is_err()
    );
    let mut bad_url = openai_request("gpt", Some("secret"));
    bad_url.base_url = Some("ftp://example.test".to_owned());
    assert!(
        service
            .save_profile("bad-url", bad_url, &retrieval)
            .await
            .is_err()
    );
    let mut bad_sampling = echo_request("echo", true);
    bad_sampling.temperature = 3.0;
    assert!(
        service
            .save_profile("bad-sampling", bad_sampling, &retrieval)
            .await
            .is_err()
    );
    let mut duplicate_headers = openai_request("gpt", None);
    duplicate_headers.headers = vec![
        ModelRequestHeader {
            name: "X-Key".to_owned(),
            value: Some("a".to_owned()),
            secret: true,
            configured: false,
        },
        ModelRequestHeader {
            name: "x-key".to_owned(),
            value: Some("b".to_owned()),
            secret: true,
            configured: false,
        },
    ];
    assert!(
        service
            .save_profile("dup-headers", duplicate_headers, &retrieval)
            .await
            .is_err()
    );
    let mut bad_header_name = openai_request("gpt", None);
    bad_header_name.headers = vec![ModelRequestHeader {
        name: "bad header".to_owned(),
        value: Some("secret".to_owned()),
        secret: true,
        configured: false,
    }];
    assert!(
        service
            .save_profile("bad-header-name", bad_header_name, &retrieval)
            .await
            .is_err()
    );
    let mut bad_header_value = openai_request("gpt", None);
    bad_header_value.headers = vec![ModelRequestHeader {
        name: "x-api-key".to_owned(),
        value: Some("line\nbreak".to_owned()),
        secret: true,
        configured: false,
    }];
    assert!(
        service
            .save_profile("bad-header-value", bad_header_value, &retrieval)
            .await
            .is_err()
    );
}

fn test_service(label: &str) -> ModelProviderConfigService {
    ModelProviderConfigService::new(test_paths(label))
}

fn test_paths(label: &str) -> RuntimePaths {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("relay-model-provider-{label}-{now}"));
    RuntimePaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        cache_dir: root.join("cache"),
        log_dir: root.join("logs"),
        temp_dir: root.join("tmp"),
        runtime_dir: root.join("run"),
        service_dir: root.join("service"),
    }
}

fn test_http_config() -> HttpConfig {
    HttpConfig::new(
        HttpBindAddress::parse("127.0.0.1:8791").expect("bind address"),
        Duration::from_millis(50),
        Duration::from_millis(50),
        crate::net::http::DEFAULT_MAX_BODY_BYTES,
        HttpProxyConfig::new(None, Vec::new(), true).expect("proxy config"),
    )
    .expect("http config")
}

async fn failing_catalog_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("catalog fixture should bind");
    let address = listener.local_addr().expect("catalog fixture address");
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("catalog request");
        stream
            .write_all(
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
            )
            .await
            .expect("catalog response");
    });
    format!("http://{address}/api.json")
}

fn openai_request(model: &str, api_key: Option<&str>) -> ModelProfileSaveRequest {
    ModelProfileSaveRequest {
        provider: ModelProviderKind::OpenAiCompatible,
        model: model.to_owned(),
        base_url: Some("https://user:pass@api.example.com/v1".to_owned()),
        api_key: api_key.map(ToOwned::to_owned),
        clear_api_key: false,
        headers: vec![ModelRequestHeader {
            name: "x-extra-secret".to_owned(),
            value: Some("header-secret".to_owned()),
            secret: true,
            configured: false,
        }],
        ssl_verify: Some(true),
        context_window: Some(128_000),
        max_tokens: Some(4096),
        temperature: default_temperature(),
        top_p: default_top_p(),
        connect_timeout_seconds: default_connect_timeout_seconds(),
        capabilities: Some(ModelCapabilities {
            input: ModelModalityMatrix {
                text: Some(true),
                image: Some(true),
                audio: None,
                video: None,
                pdf: None,
            },
            output: ModelModalityMatrix {
                text: Some(true),
                image: None,
                audio: None,
                video: None,
                pdf: None,
            },
        }),
        fallback_policy_id: Some(" same_provider_then_other_provider ".to_owned()),
        fallback_priority: 1,
        catalog_provider_id: Some(" openai ".to_owned()),
        catalog_provider_name: Some(" OpenAI ".to_owned()),
        catalog_model_name: Some(model.to_owned()),
        is_default: true,
    }
}

fn echo_request(model: &str, is_default: bool) -> ModelProfileSaveRequest {
    ModelProfileSaveRequest {
        provider: ModelProviderKind::Echo,
        model: model.to_owned(),
        base_url: None,
        api_key: None,
        clear_api_key: false,
        headers: Vec::new(),
        ssl_verify: None,
        context_window: None,
        max_tokens: None,
        temperature: 0.2,
        top_p: default_top_p(),
        connect_timeout_seconds: 5.0,
        capabilities: None,
        fallback_policy_id: None,
        fallback_priority: 0,
        catalog_provider_id: None,
        catalog_provider_name: None,
        catalog_model_name: None,
        is_default,
    }
}
