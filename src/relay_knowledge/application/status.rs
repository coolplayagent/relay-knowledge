use std::{path::Path, time::Duration};

use crate::{
    api::{AgentProtocolStatus, RuntimeStatus},
    model_provider::ModelProfileRuntimeSummary,
};

use super::RuntimeConfiguration;

pub(super) fn runtime_status(runtime: &RuntimeConfiguration) -> RuntimeStatus {
    runtime_status_with_model_profiles(
        runtime,
        ModelProfileRuntimeSummary {
            loaded: true,
            profile_count: usize::from(runtime.retrieval.remote_embedding.is_some()),
            default_profile: runtime
                .retrieval
                .remote_embedding
                .as_ref()
                .map(|_| "default".to_owned()),
            error: None,
        },
    )
}

pub(super) fn runtime_status_with_model_profiles(
    runtime: &RuntimeConfiguration,
    model_profiles: ModelProfileRuntimeSummary,
) -> RuntimeStatus {
    let network = runtime.network.current();
    let qos = runtime.network.qos_runtime().diagnostics_snapshot();

    RuntimeStatus {
        config_dir: path_string(&runtime.paths.config_dir),
        data_dir: path_string(&runtime.paths.data_dir),
        state_dir: path_string(&runtime.paths.state_dir),
        cache_dir: path_string(&runtime.paths.cache_dir),
        log_dir: path_string(&runtime.paths.log_dir),
        temp_dir: path_string(&runtime.paths.temp_dir),
        runtime_dir: path_string(&runtime.paths.runtime_dir),
        service_dir: path_string(&runtime.paths.service_dir),
        storage_topology: runtime.storage.topology.as_str().to_owned(),
        http_bind: network.http.bind_address.to_string(),
        http_request_timeout_ms: duration_millis(network.http.request_timeout),
        http_graceful_shutdown_timeout_ms: duration_millis(network.http.graceful_shutdown_timeout),
        http_max_request_body_bytes: network.http.max_request_body_bytes,
        http_proxy_configured: network.http.proxy.is_proxy_configured(),
        http_no_proxy_rules: network.http.proxy.no_proxy_rules.len(),
        http_ssl_verify: network.http.proxy.ssl_verify,
        qos_max_connections: network.qos.max_connections,
        qos_max_in_flight_requests: network.qos.max_in_flight_requests,
        qos_max_queue_depth: network.qos.max_queue_depth,
        qos_current_connections: qos.usage.connections,
        qos_current_in_flight_requests: qos.usage.in_flight_requests,
        qos_current_queued_requests: qos.usage.queued_requests,
        qos_admitted_total: qos.admitted_total,
        qos_queued_total: qos.queued_total,
        qos_rejected_total: qos.rejected_total,
        qos_timed_out_total: qos.timed_out_total,
        qos_cancelled_total: qos.cancelled_total,
        qos_dropped_total: qos.dropped_total,
        worker_embedding_endpoint_configured: runtime.workers.embedding_endpoint.is_some(),
        worker_ocr_endpoint_configured: runtime.workers.ocr_endpoint.is_some(),
        worker_vision_endpoint_configured: runtime.workers.vision_endpoint.is_some(),
        worker_extractor_endpoint_configured: runtime.workers.extractor_endpoint.is_some(),
        worker_max_in_flight: runtime.workers.max_in_flight,
        code_index_max_in_flight: runtime.workers.code_index_max_in_flight,
        silent_updates_enabled: runtime.workers.silent_updates_enabled,
        file_index_enabled: runtime.file_index.enabled,
        file_index_root_count: runtime.file_index.roots.len(),
        file_index_max_depth: runtime.file_index.max_depth,
        file_index_max_file_bytes: runtime.file_index.max_file_bytes,
        file_index_scan_interval_ms: duration_millis(runtime.file_index.scan_interval),
        file_index_scan_timeout_ms: duration_millis(runtime.file_index.scan_timeout),
        file_index_max_files_per_root: runtime.file_index.max_files_per_root,
        file_query_timeout_ms: duration_millis(runtime.file_index.query_timeout),
        semantic_backend_mode: runtime.retrieval.semantic_mode.as_str().to_owned(),
        vector_backend_mode: runtime.retrieval.vector_mode.as_str().to_owned(),
        rerank_backend_mode: runtime.retrieval.rerank.mode.as_str().to_owned(),
        rerank_model: runtime.retrieval.rerank.model.clone(),
        rerank_candidate_multiplier: runtime.retrieval.rerank.candidate_multiplier,
        rerank_max_candidates: runtime.retrieval.rerank.max_candidates,
        rerank_timeout_ms: duration_millis(runtime.retrieval.rerank.timeout),
        embedding_provider: runtime
            .retrieval
            .remote_embedding
            .as_ref()
            .map(|config| config.provider.as_str().to_owned()),
        embedding_base_url: runtime
            .retrieval
            .remote_embedding
            .as_ref()
            .map(|config| config.redacted_base_url()),
        embedding_api_key_configured: runtime.retrieval.remote_embedding.is_some(),
        text_embedding_model: runtime.retrieval.vector_model.name.clone(),
        image_embedding_model: runtime.retrieval.image_model.name.clone(),
        embedding_dimension: runtime.retrieval.vector_model.dimension,
        embedding_batch_size: runtime
            .retrieval
            .remote_embedding
            .as_ref()
            .map(|config| config.batch_size),
        embedding_timeout_ms: runtime
            .retrieval
            .remote_embedding
            .as_ref()
            .map(|config| duration_millis(config.timeout)),
        embedding_max_concurrency: runtime
            .retrieval
            .remote_embedding
            .as_ref()
            .map(|config| config.max_concurrency),
        model_profiles,
        telemetry: runtime.observability.status(),
    }
}

pub(super) fn agent_protocol_status(runtime: &RuntimeConfiguration) -> AgentProtocolStatus {
    let network = runtime.network.current();
    let mcp_enabled = runtime.agent.mcp_streamable_http_enabled;

    AgentProtocolStatus {
        mcp_streamable_http_enabled: mcp_enabled,
        mcp_endpoint: runtime.agent.mcp_endpoint.clone(),
        mcp_resources_enabled: mcp_enabled,
        mcp_prompts_enabled: mcp_enabled,
        metrics_endpoint: endpoint_child(&runtime.agent.mcp_endpoint, "metrics"),
        http_bind: network.http.bind_address.to_string(),
        allowed_origin_count: runtime.agent.mcp_allowed_origins.len(),
        mcp_allowed_origins: runtime.agent.mcp_allowed_origins.clone(),
        policy: runtime.agent.access_policy.summary(),
        audit_sink_enabled: runtime.agent.audit_sink_enabled,
        audit_log_path: path_string(&runtime.paths.agent_audit_log_file()),
        audit_queue_depth: runtime.agent.audit_queue_depth,
    }
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn endpoint_child(endpoint: &str, child: &str) -> String {
    if endpoint == "/" {
        format!("/{child}")
    } else {
        format!("{}/{child}", endpoint.trim_end_matches('/'))
    }
}
