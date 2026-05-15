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
  worker_embedding_endpoint_configured: boolean;
  worker_ocr_endpoint_configured: boolean;
  worker_vision_endpoint_configured: boolean;
  worker_extractor_endpoint_configured: boolean;
  worker_max_in_flight: number;
  silent_updates_enabled: boolean;
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
    parse_status_counts: {
      parsed: number;
      partial: number;
      text_only: number;
      failed: number;
    };
  };
  indexes: IndexStatus[];
  index_cursors: IndexCursor[];
  index_refresh: IndexRefreshDiagnostics;
  runtime: RuntimeStatus;
};

export type WorkerKind = "embedding" | "ocr" | "vision" | "extractor";

export type WorkerStatus = {
  kind: WorkerKind;
  backend_state: "fallback" | "configured" | "degraded" | "unavailable";
  endpoint_configured: boolean;
  queue_depth: number;
  running_count: number;
  retrying_count: number;
  dead_letter_count: number;
  last_error?: string;
};

export type ProposalRecord = {
  proposal_id: string;
  source_scope: string;
  kind: "evidence" | "relation" | "claim" | "event";
  state: "proposed" | "accepted" | "rejected" | "superseded";
  title: string;
  summary: string;
  payload_json: string;
  origin: string;
  confidence_basis_points: number;
  conflict_count: number;
  decided_by?: string;
  decision_reason?: string;
  created_at_ms: number;
  updated_at_ms: number;
};

export type AuditEventRecord = {
  sequence: number;
  operation: string;
  interface: string;
  request_id: string;
  trace_id: string;
  status: "started" | "completed" | "failed" | "cancelled";
  actor?: string;
  source_scope?: string;
  graph_version: number;
  detail_json: string;
  message?: string;
  created_at_ms: number;
};

export type ServiceOperatorStatus = {
  state: "disabled" | "enabled" | "paused" | "degraded" | "failed";
  silent_updates_enabled: boolean;
  allowed_scopes: string[];
  last_run_at_ms?: number;
  next_retry_at_ms?: number;
  last_error?: string;
  updated_at_ms: number;
};

export type ServiceStatusResponse = {
  metadata: ApiMetadata;
  service_name: string;
  mode: string;
  background_enabled: boolean;
  silent_updates_enabled: boolean;
  service_definition_path: string;
  index_refresh: IndexRefreshDiagnostics;
  agent_protocols: {
    mcp_streamable_http_enabled: boolean;
    mcp_resources_enabled: boolean;
    mcp_prompts_enabled: boolean;
    acp_local_adapter_enabled: boolean;
    metrics_enabled: boolean;
  };
  operator: ServiceOperatorStatus;
  workers: WorkerStatus[];
  proposal_backlog: number;
  audit_sink: {
    durable: boolean;
    event_count: number;
    last_error?: string;
  };
};

export type GraphCanvasKind = "knowledge" | "code" | "mixed";

export type GraphCanvasNode = {
  id: string;
  kind: string;
  label: string;
  subtitle?: string;
  source_scope?: string;
  graph_version: number;
  weight: number;
  status?: string;
  details: Record<string, string>;
};

export type GraphCanvasEdge = {
  id: string;
  kind: string;
  source: string;
  target: string;
  label: string;
  graph_version: number;
  confidence_basis_points?: number;
  evidence_count?: number;
  details: Record<string, string>;
};

export type GraphCanvasResponse = {
  metadata: ApiMetadata;
  nodes: GraphCanvasNode[];
  edges: GraphCanvasEdge[];
  summary: {
    kind: GraphCanvasKind;
    node_count: number;
    edge_count: number;
    truncated: boolean;
    available_kinds: string[];
  };
};

export type WebOperationSnapshot = {
  id: string;
  name: string;
  command: string;
  payload: Record<string, unknown>;
  createdAt: string;
};

export type WebOperationExecuteRequest = {
  snapshot: WebOperationSnapshot;
};

export type WebOperationExecuteResponse = {
  metadata: ApiMetadata;
  operation: string;
  name: string;
  command: string;
  result: unknown;
};
