use std::{path::Path, time::Duration};

use crate::api::{AgentProtocolStatus, RuntimeStatus};

use super::RuntimeConfiguration;

pub(super) fn runtime_status(runtime: &RuntimeConfiguration) -> RuntimeStatus {
    let network = runtime.network.current();

    RuntimeStatus {
        config_dir: path_string(&runtime.paths.config_dir),
        data_dir: path_string(&runtime.paths.data_dir),
        state_dir: path_string(&runtime.paths.state_dir),
        cache_dir: path_string(&runtime.paths.cache_dir),
        log_dir: path_string(&runtime.paths.log_dir),
        temp_dir: path_string(&runtime.paths.temp_dir),
        runtime_dir: path_string(&runtime.paths.runtime_dir),
        service_dir: path_string(&runtime.paths.service_dir),
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
        worker_embedding_endpoint_configured: runtime.workers.embedding_endpoint.is_some(),
        worker_ocr_endpoint_configured: runtime.workers.ocr_endpoint.is_some(),
        worker_vision_endpoint_configured: runtime.workers.vision_endpoint.is_some(),
        worker_extractor_endpoint_configured: runtime.workers.extractor_endpoint.is_some(),
        worker_max_in_flight: runtime.workers.max_in_flight,
        silent_updates_enabled: runtime.workers.silent_updates_enabled,
        semantic_backend_mode: runtime.retrieval.semantic_mode.as_str().to_owned(),
        vector_backend_mode: runtime.retrieval.vector_mode.as_str().to_owned(),
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
    }
}

pub(super) fn agent_protocol_status(runtime: &RuntimeConfiguration) -> AgentProtocolStatus {
    let network = runtime.network.current();

    AgentProtocolStatus {
        mcp_streamable_http_enabled: runtime.agent.mcp_streamable_http_enabled,
        mcp_endpoint: runtime.agent.mcp_endpoint.clone(),
        http_bind: network.http.bind_address.to_string(),
        allowed_origin_count: runtime.agent.mcp_allowed_origins.len(),
        policy: runtime.agent.access_policy.summary(),
    }
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
