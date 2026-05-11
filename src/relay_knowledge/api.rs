//! Stable API contracts shared by CLI, Web, and future service adapters.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::domain::GraphVersion;

/// External interface that initiated a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InterfaceKind {
    /// Command-line interface adapter.
    Cli,
    /// Web user interface adapter.
    Web,
    /// Future HTTP or RPC API adapter.
    Api,
}

/// Request-scoped identity propagated through application services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestContext {
    pub interface: InterfaceKind,
    pub request_id: String,
    pub trace_id: String,
}

impl RequestContext {
    /// Creates a request context for an interface with generated local IDs.
    pub fn for_interface(interface: InterfaceKind) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());

        Self {
            interface,
            request_id: format!("req-{nanos}"),
            trace_id: format!("trace-{nanos}"),
        }
    }

    /// Creates a request context with explicit IDs for tests and adapter bridges.
    pub fn with_ids(
        interface: InterfaceKind,
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            interface,
            request_id: request_id.into(),
            trace_id: trace_id.into(),
        }
    }
}

/// Common metadata that every successful API response must carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiMetadata {
    pub trace_id: String,
    pub request_id: String,
    pub graph_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_graph_version: Option<u64>,
    pub stale: bool,
}

impl ApiMetadata {
    /// Builds response metadata for graph-only operations.
    pub fn graph_only(context: &RequestContext, graph_version: GraphVersion) -> Self {
        Self {
            trace_id: context.trace_id.clone(),
            request_id: context.request_id.clone(),
            graph_version: graph_version.get(),
            index_version: None,
            indexed_graph_version: None,
            stale: false,
        }
    }
}

/// Stable error categories used across interface adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidArgument,
    Internal,
}

/// API error shape suitable for JSON and streaming output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error_kind: ErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ApiMetadata>,
}

impl ApiError {
    /// Creates an invalid argument error.
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self {
            error_kind: ErrorKind::InvalidArgument,
            message: message.into(),
            metadata: None,
        }
    }
}

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
}

/// Minimal project status response exposed through the unified API layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectStatusResponse {
    pub project_name: String,
    pub metadata: ApiMetadata,
    pub runtime: RuntimeStatus,
}

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
            error_kind: None,
            metadata: Some(response.metadata.clone()),
        }
    }
}
