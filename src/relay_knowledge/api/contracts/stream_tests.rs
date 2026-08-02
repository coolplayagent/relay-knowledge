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
            storage_topology: "single_sqlite".to_owned(),
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
            qos_current_connections: 0,
            qos_current_in_flight_requests: 0,
            qos_current_queued_requests: 0,
            qos_admitted_total: 0,
            qos_queued_total: 0,
            qos_rejected_total: 0,
            qos_timed_out_total: 0,
            qos_cancelled_total: 0,
            qos_dropped_total: 0,
            worker_embedding_endpoint_configured: false,
            worker_ocr_endpoint_configured: false,
            worker_vision_endpoint_configured: false,
            worker_extractor_endpoint_configured: false,
            worker_max_in_flight: 2,
            code_index_max_in_flight: 2,
            silent_updates_enabled: false,
            file_index_enabled: false,
            file_index_root_count: 0,
            file_index_max_depth: 32,
            file_index_max_file_bytes: 512 * 1024 * 1024,
            file_index_scan_interval_ms: 900_000,
            file_index_scan_timeout_ms: 300_000,
            file_index_max_files_per_root: 50_000,
            file_query_timeout_ms: 750,
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
            model_profiles: crate::model_provider::ModelProfileRuntimeSummary {
                loaded: true,
                profile_count: 0,
                default_profile: None,
                error: None,
            },
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
