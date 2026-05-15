use serde::{Deserialize, Serialize};

use super::{ApiMetadata, ErrorKind, ProjectStatusResponse, RuntimeStatus};

/// Stream event categories for newline-delimited JSON output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamEventKind {
    Started,
    Progress,
    Item,
    Completed,
    Failed,
}

/// A single streaming API event. Each serialized event is one NDJSON line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiStreamEvent {
    pub event: StreamEventKind,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<ErrorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ApiMetadata>,
}

impl ApiStreamEvent {
    /// Creates a stream event for the project status operation.
    pub fn project_status(
        event: StreamEventKind,
        response: &ProjectStatusResponse,
        message: Option<&str>,
    ) -> Self {
        Self {
            event,
            operation: "project.status".to_owned(),
            message: message.map(str::to_owned),
            project_name: (event == StreamEventKind::Item).then(|| response.project_name.clone()),
            runtime: (event == StreamEventKind::Item).then(|| response.runtime.clone()),
            payload: None,
            error_kind: None,
            metadata: Some(response.metadata.clone()),
        }
    }

    /// Creates a generic streaming event for non-status operations.
    pub fn operation(
        event: StreamEventKind,
        operation: impl Into<String>,
        metadata: ApiMetadata,
        message: Option<&str>,
        payload: Option<serde_json::Value>,
    ) -> Self {
        Self {
            event,
            operation: operation.into(),
            message: message.map(str::to_owned),
            project_name: None,
            runtime: None,
            payload,
            error_kind: None,
            metadata: Some(metadata),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        api::{InterfaceKind, RequestContext},
        domain::GraphVersion,
    };

    #[test]
    fn project_status_event_only_attaches_payload_to_item() {
        let context = RequestContext::with_ids(InterfaceKind::Cli, "req", "trace");
        let response = ProjectStatusResponse {
            project_name: "relay-knowledge".to_owned(),
            metadata: ApiMetadata::graph_only(&context, GraphVersion::ZERO),
            runtime: RuntimeStatus {
                config_dir: "/config".to_owned(),
                data_dir: "/data".to_owned(),
                state_dir: "/state".to_owned(),
                cache_dir: "/cache".to_owned(),
                log_dir: "/logs".to_owned(),
                temp_dir: "/tmp".to_owned(),
                runtime_dir: "/run".to_owned(),
                service_dir: "/service".to_owned(),
                http_bind: "127.0.0.1:8791".to_owned(),
                http_request_timeout_ms: 30000,
                http_graceful_shutdown_timeout_ms: 10000,
                http_max_request_body_bytes: 1024,
                http_proxy_configured: false,
                http_no_proxy_rules: 0,
                http_ssl_verify: true,
                qos_max_connections: 1,
                qos_max_in_flight_requests: 1,
                qos_max_queue_depth: 1,
                worker_embedding_endpoint_configured: false,
                worker_ocr_endpoint_configured: false,
                worker_vision_endpoint_configured: false,
                worker_extractor_endpoint_configured: false,
                worker_max_in_flight: 2,
                silent_updates_enabled: false,
                semantic_backend_mode: "local".to_owned(),
                vector_backend_mode: "local".to_owned(),
                rerank_backend_mode: "local".to_owned(),
                rerank_model: Some("relay-local-deterministic-rerank-v1".to_owned()),
                rerank_candidate_multiplier: 4,
                rerank_max_candidates: 64,
                rerank_timeout_ms: 100,
                embedding_provider: None,
                embedding_base_url: None,
                embedding_api_key_configured: false,
                text_embedding_model: "relay-local-hash-ann-v1".to_owned(),
                image_embedding_model: "relay-local-image-hash-v1".to_owned(),
                embedding_dimension: 16,
                embedding_batch_size: None,
                embedding_timeout_ms: None,
                embedding_max_concurrency: None,
                telemetry: crate::observability::ObservabilityRuntime::new(
                    crate::observability::TelemetryConfig::from_environment(
                        &crate::env::TelemetryEnvOverrides::default(),
                    ),
                )
                .status(),
            },
        };

        let started =
            ApiStreamEvent::project_status(StreamEventKind::Started, &response, Some("starting"));
        let item = ApiStreamEvent::project_status(StreamEventKind::Item, &response, None);

        assert_eq!(started.project_name, None);
        assert_eq!(started.message, Some("starting".to_owned()));
        assert_eq!(item.project_name, Some("relay-knowledge".to_owned()));
        assert!(item.runtime.is_some());
    }

    #[test]
    fn operation_event_carries_generic_payload() {
        let context = RequestContext::with_ids(InterfaceKind::Api, "req", "trace");
        let metadata = ApiMetadata::graph_only(&context, GraphVersion::ZERO);

        let event = ApiStreamEvent::operation(
            StreamEventKind::Item,
            "health",
            metadata,
            None,
            Some(json!({"healthy": true})),
        );

        assert_eq!(event.operation, "health");
        assert_eq!(event.payload, Some(json!({"healthy": true})));
    }
}
