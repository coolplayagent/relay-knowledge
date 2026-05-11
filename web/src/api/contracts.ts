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

export type HealthResponse = {
  metadata: ApiMetadata;
  healthy: boolean;
  graph: {
    graph_version: number;
    entity_count: number;
    evidence_count: number;
    mutation_count: number;
  };
  indexes: IndexStatus[];
  runtime: RuntimeStatus;
};
