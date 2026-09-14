use serde::{Deserialize, Serialize};

use crate::{
    api::{AgentProtocolStatus, ApiMetadata, RuntimeStatus, WatcherDiagnostics},
    domain::{
        CodeRepositoryTotals, FileIndexDiagnostics, GraphInspection, IndexCursor, IndexKind,
        IndexRefreshDiagnostics, IndexStatus, ServiceDefinitionPlan, ServiceOperatorStatus,
        WorkerStatus,
    },
};

use super::{AuditSinkStatus, CodeIndexWorkerStatus};

/// Service manager status surfaced without exposing platform-specific handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatusResponse {
    pub metadata: ApiMetadata,
    pub service_name: String,
    pub mode: String,
    pub background_enabled: bool,
    pub silent_updates_enabled: bool,
    pub service_definition_path: String,
    pub storage: StorageTopologyDiagnostics,
    pub index_refresh: IndexRefreshDiagnostics,
    pub file_index: FileIndexDiagnostics,
    pub agent_protocols: AgentProtocolStatus,
    pub operator: ServiceOperatorStatus,
    pub workers: Vec<WorkerStatus>,
    pub code_index_workers: CodeIndexWorkerStatus,
    pub proposal_backlog: usize,
    pub audit_sink: AuditSinkStatus,
    #[serde(default = "WatcherDiagnostics::default_disabled")]
    pub watcher: WatcherDiagnostics,
}

/// Storage topology and shard-catalog diagnostics surfaced by the control plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageTopologyDiagnostics {
    pub topology: String,
    pub control_database_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_shards_dir: Option<String>,
    pub shard_catalog_active: bool,
    pub active_shard_count: usize,
    pub staged_shard_count: usize,
    pub missing_shard_count: usize,
    pub runtime_state_paths: Vec<String>,
    pub shards: Vec<StorageShardDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// One partitioned SQLite shard reported through storage diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageShardDiagnostics {
    pub repository_id: String,
    pub state: String,
    pub shard_locator: String,
    pub resolved_path: String,
    pub source_scope_count: usize,
    pub exists: bool,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Control-plane storage topology response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageTopologyResponse {
    pub metadata: ApiMetadata,
    pub storage: StorageTopologyDiagnostics,
}

/// Service definition write response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceDefinitionWriteResponse {
    pub metadata: ApiMetadata,
    pub plan: ServiceDefinitionPlan,
    pub written: bool,
}

/// Service silent-update operator response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceOperatorResponse {
    pub metadata: ApiMetadata,
    pub operator: ServiceOperatorStatus,
}

/// Startup recovery report for resident service mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceRecoveryReport {
    pub metadata: ApiMetadata,
    pub graph_version: u64,
    pub stale_index_kinds: Vec<IndexKind>,
    pub refreshed_index_kinds: Vec<IndexKind>,
    pub index_lag_max: u64,
    pub task_queue_depth: usize,
    pub dead_letter_count: usize,
    pub heartbeat_state: String,
}

/// Aggregated health response for CLI/Web/service diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub metadata: ApiMetadata,
    pub healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub storage: StorageTopologyDiagnostics,
    pub graph: GraphInspection,
    pub repository_code_totals: CodeRepositoryTotals,
    pub indexes: Vec<IndexStatus>,
    pub index_cursors: Vec<IndexCursor>,
    pub index_refresh: IndexRefreshDiagnostics,
    pub file_index: FileIndexDiagnostics,
    pub runtime: RuntimeStatus,
}

/// Remote embedding provider probe response with secret-free diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingProviderProbeResponse {
    pub metadata: ApiMetadata,
    pub ok: bool,
    pub provider: Option<String>,
    pub model: String,
    pub dimension: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}
