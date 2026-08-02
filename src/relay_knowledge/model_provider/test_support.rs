use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncWriteExt;

use super::profile::{default_connect_timeout_seconds, default_temperature, default_top_p};
use super::*;
use crate::{
    net::http::{HttpBindAddress, HttpConfig, HttpProxyConfig},
    retrieval::{
        DEFAULT_EMBEDDING_BATCH_SIZE, DEFAULT_EMBEDDING_MAX_CONCURRENCY, DEFAULT_EMBEDDING_TIMEOUT,
        EmbeddingProviderKind, ReadModelBackendConfig, ReadModelBackendMode, RemoteEmbeddingConfig,
    },
};

pub(super) fn test_service(label: &str) -> ModelProviderConfigService {
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

pub(super) fn test_http_config() -> HttpConfig {
    HttpConfig::new(
        HttpBindAddress::parse("127.0.0.1:8791").expect("bind address"),
        Duration::from_millis(50),
        Duration::from_millis(50),
        crate::net::http::DEFAULT_MAX_BODY_BYTES,
        HttpProxyConfig::new(None, Vec::new(), true).expect("proxy config"),
    )
    .expect("http config")
}

pub(super) fn remote_retrieval() -> ReadModelBackendConfig {
    let mut config = ReadModelBackendConfig::local();
    config.semantic_mode = ReadModelBackendMode::External;
    config.vector_mode = ReadModelBackendMode::External;
    config.vector_model.name = "text-embedding-3-small".to_owned();
    config.remote_embedding = Some(RemoteEmbeddingConfig {
        provider: EmbeddingProviderKind::OpenAiCompatible,
        base_url: "https://api.openai.example/v1".to_owned(),
        api_key: "env-secret".to_owned(),
        batch_size: DEFAULT_EMBEDDING_BATCH_SIZE,
        timeout: DEFAULT_EMBEDDING_TIMEOUT,
        max_concurrency: DEFAULT_EMBEDDING_MAX_CONCURRENCY,
    });
    config
}

pub(super) async fn failing_catalog_url() -> String {
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

pub(super) fn openai_request(model: &str, api_key: Option<&str>) -> ModelProfileSaveRequest {
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

pub(super) fn echo_request(model: &str, is_default: bool) -> ModelProfileSaveRequest {
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
