use serde::{Deserialize, Serialize};

use super::ApiMetadata;

/// Resolved runtime paths and network budgets exposed for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub config_dir: String,
    pub data_dir: String,
    pub state_dir: String,
    pub cache_dir: String,
    pub log_dir: String,
    pub temp_dir: String,
    pub runtime_dir: String,
    pub service_dir: String,
    pub http_bind: String,
    pub http_request_timeout_ms: u64,
    pub http_graceful_shutdown_timeout_ms: u64,
    pub http_max_request_body_bytes: u64,
    pub http_proxy_configured: bool,
    pub http_no_proxy_rules: usize,
    pub http_ssl_verify: bool,
    pub qos_max_connections: usize,
    pub qos_max_in_flight_requests: usize,
    pub qos_max_queue_depth: usize,
    pub semantic_backend_mode: String,
    pub vector_backend_mode: String,
    pub embedding_provider: Option<String>,
    pub embedding_base_url: Option<String>,
    pub embedding_api_key_configured: bool,
    pub text_embedding_model: String,
    pub image_embedding_model: String,
    pub embedding_dimension: u32,
    pub embedding_batch_size: Option<usize>,
    pub embedding_timeout_ms: Option<u64>,
    pub embedding_max_concurrency: Option<usize>,
}

/// Minimal project status response exposed through the unified API layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectStatusResponse {
    pub project_name: String,
    pub metadata: ApiMetadata,
    pub runtime: RuntimeStatus,
}
