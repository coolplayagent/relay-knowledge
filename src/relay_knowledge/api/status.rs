use serde::{Deserialize, Serialize};

use super::ApiMetadata;
use crate::model_provider::ModelProfileRuntimeSummary;
use crate::observability::TelemetryStatus;

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
    pub storage_topology: String,
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
    pub worker_embedding_endpoint_configured: bool,
    pub worker_ocr_endpoint_configured: bool,
    pub worker_vision_endpoint_configured: bool,
    pub worker_extractor_endpoint_configured: bool,
    pub worker_max_in_flight: usize,
    pub code_index_max_in_flight: usize,
    pub silent_updates_enabled: bool,
    pub file_index_enabled: bool,
    pub file_index_root_count: usize,
    pub file_index_max_depth: usize,
    pub file_index_max_file_bytes: u64,
    pub file_index_scan_interval_ms: u64,
    pub file_index_scan_timeout_ms: u64,
    pub file_index_max_files_per_root: usize,
    pub file_query_timeout_ms: u64,
    pub semantic_backend_mode: String,
    pub vector_backend_mode: String,
    pub rerank_backend_mode: String,
    pub rerank_model: Option<String>,
    pub rerank_candidate_multiplier: usize,
    pub rerank_max_candidates: usize,
    pub rerank_timeout_ms: u64,
    pub embedding_provider: Option<String>,
    pub embedding_base_url: Option<String>,
    pub embedding_api_key_configured: bool,
    pub text_embedding_model: String,
    pub image_embedding_model: String,
    pub embedding_dimension: u32,
    pub embedding_batch_size: Option<usize>,
    pub embedding_timeout_ms: Option<u64>,
    pub embedding_max_concurrency: Option<usize>,
    pub model_profiles: ModelProfileRuntimeSummary,
    pub telemetry: TelemetryStatus,
}

/// Minimal project status response exposed through the unified API layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectStatusResponse {
    pub project_name: String,
    pub metadata: ApiMetadata,
    pub runtime: RuntimeStatus,
}
