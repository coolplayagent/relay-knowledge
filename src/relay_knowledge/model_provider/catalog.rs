use serde_json::Value;
use tokio::fs;

use super::{
    ModelCatalogCache, ModelCatalogResult, ModelProviderConfigService, ModelProviderError,
    helpers::*,
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
