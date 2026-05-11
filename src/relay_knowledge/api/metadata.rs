use serde::{Deserialize, Serialize};

use crate::domain::GraphVersion;

use super::RequestContext;

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
