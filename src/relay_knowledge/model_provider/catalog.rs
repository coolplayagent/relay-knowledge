use serde_json::Value;
use tokio::fs;

use super::{
    ModelCapabilities, ModelCatalogCache, ModelCatalogModel, ModelCatalogProvider,
    ModelCatalogResult, ModelProviderConfigService, ModelProviderError, ModelProviderKind,
    connectivity::{now_millis, status_error_code},
    persistence::write_json,
};
use crate::net::{
    http::{HttpConfig, send_request_with_qos},
    qos::{QosPolicy, QosRuntime},
};

impl ModelProviderConfigService {
    pub async fn catalog(
        &self,
        http: &HttpConfig,
        refresh: bool,
    ) -> Result<ModelCatalogResult, ModelProviderError> {
        let qos = QosRuntime::default();
        let policy = QosPolicy::new(
            crate::net::qos::DEFAULT_MAX_CONNECTIONS,
            crate::net::qos::DEFAULT_MAX_IN_FLIGHT_REQUESTS,
            crate::net::qos::DEFAULT_MAX_QUEUE_DEPTH,
        )
        .expect("default QoS policy should validate");
        self.catalog_with_qos(http, &qos, &policy, refresh).await
    }

    pub async fn catalog_with_qos(
        &self,
        http: &HttpConfig,
        qos: &QosRuntime,
        policy: &QosPolicy,
        refresh: bool,
    ) -> Result<ModelCatalogResult, ModelProviderError> {
        let cached = self.load_catalog_cache().await?;
        if !refresh {
            return Ok(cached
                .map(|cache| catalog_result_from_cache(cache, true, None, None))
                .unwrap_or_else(builtin_catalog_result));
        }

        let fetched = self.fetch_catalog(http, qos, policy).await;
        match fetched {
            Ok(result) if result.ok => {
                let cache = ModelCatalogCache {
                    source_url: result.source_url.clone(),
                    fetched_at_ms: result.fetched_at_ms.unwrap_or_else(now_millis),
                    providers: result.providers.clone(),
                };
                let _ = self.write_catalog_cache(&cache).await;
                Ok(result)
            }
            Ok(result) => {
                let fallback_error_code = result.error_code.clone();
                let fallback_error_message = result.error_message.clone();
                let source_url = result.source_url.clone();
                let fetched_at_ms = result.fetched_at_ms;
                Ok(cached
                    .map(|cache| {
                        catalog_result_from_cache(
                            cache,
                            false,
                            fallback_error_code.clone(),
                            fallback_error_message.clone(),
                        )
                    })
                    .unwrap_or_else(|| ModelCatalogResult {
                        ok: false,
                        source_url,
                        fetched_at_ms,
                        cache_age_seconds: None,
                        stale: true,
                        providers: builtin_catalog_providers(),
                        error_code: fallback_error_code,
                        error_message: fallback_error_message,
                    }))
            }
            Err(error) => Ok(cached
                .map(|cache| {
                    catalog_result_from_cache(
                        cache,
                        false,
                        Some("network_error".to_owned()),
                        Some(error.to_string()),
                    )
                })
                .unwrap_or_else(|| ModelCatalogResult {
                    ok: false,
                    source_url: self.catalog_source_url.clone(),
                    fetched_at_ms: None,
                    cache_age_seconds: None,
                    stale: true,
                    providers: builtin_catalog_providers(),
                    error_code: Some("network_error".to_owned()),
                    error_message: Some(error.to_string()),
                })),
        }
    }

    async fn load_catalog_cache(&self) -> Result<Option<ModelCatalogCache>, ModelProviderError> {
        match fs::read_to_string(self.paths.model_catalog_cache_file()).await {
            Ok(raw) => serde_json::from_str(&raw)
                .map(Some)
                .map_err(ModelProviderError::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ModelProviderError::from(error)),
        }
    }

    pub(super) async fn write_catalog_cache(
        &self,
        cache: &ModelCatalogCache,
    ) -> Result<(), ModelProviderError> {
        write_json(self.paths.model_catalog_cache_file(), cache).await
    }

    async fn fetch_catalog(
        &self,
        http: &HttpConfig,
        qos: &QosRuntime,
        policy: &QosPolicy,
    ) -> Result<ModelCatalogResult, ModelProviderError> {
        let client = crate::net::http::outbound_json_client(http)
            .map_err(|error| ModelProviderError::Network(error.to_string()))?;
        let response = send_request_with_qos(
            qos,
            policy,
            client
                .get(&self.catalog_source_url)
                .timeout(http.request_timeout),
        )
        .await
        .map_err(|error| ModelProviderError::Network(error.to_string()))?;
        if !response.status().is_success() {
            return Ok(ModelCatalogResult {
                ok: false,
                source_url: self.catalog_source_url.clone(),
                fetched_at_ms: None,
                cache_age_seconds: None,
                stale: true,
                providers: Vec::new(),
                error_code: Some(status_error_code(response.status().as_u16()).to_owned()),
                error_message: Some(format!("catalog returned HTTP {}", response.status())),
            });
        }
        let payload = response
            .json::<Value>()
            .await
            .map_err(|error| ModelProviderError::Json(error.to_string()))?;
        Ok(ModelCatalogResult {
            ok: true,
            source_url: self.catalog_source_url.clone(),
            fetched_at_ms: Some(now_millis()),
            cache_age_seconds: Some(0),
            stale: false,
            providers: parse_catalog_payload(&payload),
            error_code: None,
            error_message: None,
        })
    }
}
pub(super) fn parse_catalog_payload(payload: &Value) -> Vec<ModelCatalogProvider> {
    let providers = payload
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let parsed = providers
        .iter()
        .filter_map(parse_catalog_provider)
        .collect::<Vec<_>>();
    if parsed.is_empty() {
        builtin_catalog_providers()
    } else {
        parsed
    }
}

pub(super) fn parse_catalog_provider(value: &Value) -> Option<ModelCatalogProvider> {
    let id = value.get("id").and_then(Value::as_str)?.to_owned();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_owned();
    let runtime_provider = match value
        .get("runtime_provider")
        .or_else(|| value.get("provider"))
        .and_then(Value::as_str)
        .unwrap_or("openai_compatible")
    {
        "anthropic" => ModelProviderKind::Anthropic,
        "bigmodel" => ModelProviderKind::Bigmodel,
        "minimax" => ModelProviderKind::Minimax,
        "maas" => ModelProviderKind::Maas,
        "codeagent" => ModelProviderKind::Codeagent,
        "echo" => ModelProviderKind::Echo,
        _ => ModelProviderKind::OpenAiCompatible,
    };
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_catalog_model)
        .collect();
    Some(ModelCatalogProvider {
        id,
        name,
        runtime_provider,
        api: value
            .get("api")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        doc: value
            .get("doc")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        env: value
            .get("env")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        models,
    })
}

pub(super) fn parse_catalog_model(value: &Value) -> Option<ModelCatalogModel> {
    let id = value
        .get("id")
        .or_else(|| value.get("model"))
        .and_then(Value::as_str)?
        .to_owned();
    Some(ModelCatalogModel {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_owned(),
        id,
        family: value
            .get("family")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        context_window: value
            .get("context_window")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        output_limit: value
            .get("output_limit")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        capabilities: ModelCapabilities::default(),
    })
}

pub(super) fn builtin_catalog_result() -> ModelCatalogResult {
    ModelCatalogResult {
        ok: true,
        source_url: "builtin".to_owned(),
        fetched_at_ms: Some(now_millis()),
        cache_age_seconds: Some(0),
        stale: false,
        providers: builtin_catalog_providers(),
        error_code: None,
        error_message: None,
    }
}

pub(super) fn builtin_catalog_providers() -> Vec<ModelCatalogProvider> {
    vec![
        catalog_provider(
            "openai",
            "OpenAI-compatible",
            ModelProviderKind::OpenAiCompatible,
            &["gpt-4.1", "gpt-4.1-mini", "text-embedding-3-small"],
        ),
        catalog_provider(
            "anthropic",
            "Anthropic",
            ModelProviderKind::Anthropic,
            &["claude-sonnet-4-5", "claude-haiku-4-5"],
        ),
        catalog_provider("echo", "Echo", ModelProviderKind::Echo, &["echo"]),
    ]
}

pub(super) fn catalog_provider(
    id: &str,
    name: &str,
    runtime_provider: ModelProviderKind,
    models: &[&str],
) -> ModelCatalogProvider {
    ModelCatalogProvider {
        id: id.to_owned(),
        name: name.to_owned(),
        runtime_provider,
        api: None,
        doc: None,
        env: Vec::new(),
        models: models
            .iter()
            .map(|model| ModelCatalogModel {
                id: (*model).to_owned(),
                name: (*model).to_owned(),
                family: None,
                context_window: None,
                output_limit: None,
                capabilities: ModelCapabilities::default(),
            })
            .collect(),
    }
}

pub(super) fn catalog_result_from_cache(
    cache: ModelCatalogCache,
    ok: bool,
    error_code: Option<String>,
    error_message: Option<String>,
) -> ModelCatalogResult {
    let age = now_millis().saturating_sub(cache.fetched_at_ms) / 1000;
    ModelCatalogResult {
        ok,
        source_url: cache.source_url,
        fetched_at_ms: Some(cache.fetched_at_ms),
        cache_age_seconds: Some(age),
        stale: !ok,
        providers: cache.providers,
        error_code,
        error_message,
    }
}
