use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

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
    /// Model Context Protocol adapter.
    Mcp,
    /// Agent Client Protocol adapter.
    Acp,
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
