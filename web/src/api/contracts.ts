export type ApiMetadata = {
  trace_id: string;
  request_id: string;
  graph_version: number;
  index_version?: number;
  indexed_graph_version?: number;
  stale: boolean;
};

export type RuntimeStatus = {
  config_dir: string;
  data_dir: string;
  state_dir: string;
  cache_dir: string;
  log_dir: string;
  temp_dir: string;
  runtime_dir: string;
  service_dir: string;
  http_bind: string;
  http_request_timeout_ms: number;
  http_graceful_shutdown_timeout_ms: number;
  http_max_request_body_bytes: number;
  http_proxy_configured: boolean;
  http_no_proxy_rules: number;
  http_ssl_verify: boolean;
  qos_max_connections: number;
  qos_max_in_flight_requests: number;
  qos_max_queue_depth: number;
  semantic_backend_mode: "local" | "external" | "disabled";
  vector_backend_mode: "local" | "external" | "disabled";
  embedding_provider?: "openai_compatible" | "echo";
  embedding_base_url?: string;
  embedding_api_key_configured: boolean;
  text_embedding_model: string;
  image_embedding_model: string;
  embedding_dimension: number;
  embedding_batch_size?: number;
  embedding_timeout_ms?: number;
  embedding_max_concurrency?: number;
};

export type ProjectStatusResponse = {
  project_name: string;
  metadata: ApiMetadata;
  runtime: RuntimeStatus;
};

export type IndexStatus = {
  kind: "bm25" | "semantic" | "vector";
  index_version: number;
  indexed_graph_version: number;
  state: "fresh" | "stale" | "failed" | "paused";
  last_error?: string;
};

export type IndexCursor = IndexStatus & {
  source_scope: string;
  modality: "text" | "image" | "layout" | "table";
  source_hash?: string;
  backend_cursor?: string;
  model_name?: string;
  model_dimension?: number;
};

export type IndexStalenessReason = {
  kind: "bm25" | "semantic" | "vector";
  source_scope?: string;
  modality?: "text" | "image" | "layout" | "table";
  reason: string;
  lag_versions: number;
  last_error?: string;
};

export type IndexRefreshDiagnostics = {
  queue_depth: number;
  running_count: number;
  retrying_count: number;
  dead_letter_count: number;
  oldest_unfinished_age_ms?: number;
  index_lag_by_kind: Array<{
    kind: "bm25" | "semantic" | "vector";
    lag_versions: number;
  }>;
  max_index_lag_versions: number;
  stale_index_count: number;
  stale_reasons: IndexStalenessReason[];
};

export type HealthResponse = {
  metadata: ApiMetadata;
  healthy: boolean;
  graph: {
    graph_version: number;
    entity_count: number;
    evidence_count: number;
    relation_count: number;
    claim_count: number;
    event_count: number;
    mutation_count: number;
    code_file_count: number;
    code_symbol_count: number;
    code_reference_count: number;
    code_chunk_count: number;
    code_parse_status_counts: {
      parsed: number;
      partial: number;
      text_only: number;
      failed: number;
    };
  };
  repository_code_totals: {
    repository_count: number;
    indexed_file_count: number;
    symbol_count: number;
    reference_count: number;
    chunk_count: number;
    degraded_file_count: number;
  };
  indexes: IndexStatus[];
  index_cursors: IndexCursor[];
  index_refresh: IndexRefreshDiagnostics;
  runtime: RuntimeStatus;
};
