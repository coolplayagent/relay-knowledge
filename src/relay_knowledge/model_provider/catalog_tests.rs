use serde_json::json;

use super::*;
use crate::model_provider::test_support::{failing_catalog_url, test_http_config, test_service};

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

#[test]
fn unknown_catalog_payload_uses_builtin_providers() {
    let providers = parse_catalog_payload(&json!({"unexpected": true}));

    assert!(providers.iter().any(|provider| provider.id == "openai"));
}

#[test]
fn maps_catalog_payload_and_cache_fallback() {
    let providers = parse_catalog_payload(&json!({
        "providers": [
            {
                "id": "anthropic",
                "name": "Anthropic",
                "provider": "anthropic",
                "api": "https://api.anthropic.com",
                "doc": "https://docs.example.com",
                "env": ["ANTHROPIC_API_KEY"],
                "models": [
                    {
                        "model": "claude",
                        "name": "Claude",
                        "family": "claude",
                        "context_window": 200000,
                        "output_limit": 8192
                    }
                ]
            },
            {"id": "unknown", "provider": "unknown", "models": [{"id": "u"}]}
        ]
    }));
    assert_eq!(providers[0].runtime_provider, ModelProviderKind::Anthropic);
    assert_eq!(providers[0].models[0].family.as_deref(), Some("claude"));
    assert_eq!(
        providers[1].runtime_provider,
        ModelProviderKind::OpenAiCompatible
    );
    assert_eq!(parse_catalog_model(&json!({"missing": true})), None);

    let cache = ModelCatalogCache {
        source_url: "cache".to_owned(),
        fetched_at_ms: now_millis(),
        providers: providers.clone(),
    };
    let cached = catalog_result_from_cache(
        cache,
        false,
        Some("network_error".to_owned()),
        Some("offline".to_owned()),
    );
    assert!(cached.stale);
    assert_eq!(cached.providers, providers);
}
