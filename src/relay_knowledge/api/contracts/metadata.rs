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
    /// Lightweight reference for explicit feedback; generating it performs no I/O.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<FeedbackHandle>,
}

/// Correlates feedback with existing operation metadata without copying logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackHandle {
    pub schema_version: u32,
    pub trace_id: String,
    pub request_id: String,
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
            feedback: Some(FeedbackHandle {
                schema_version: 1,
                trace_id: context.trace_id.clone(),
                request_id: context.request_id.clone(),
            }),
        }
    }

    /// Builds response metadata for operations that used derived indexes.
    pub fn indexed(
        context: &RequestContext,
        graph_version: GraphVersion,
        index_version: Option<u64>,
        indexed_graph_version: Option<GraphVersion>,
        stale: bool,
    ) -> Self {
        Self {
            trace_id: context.trace_id.clone(),
            request_id: context.request_id.clone(),
            graph_version: graph_version.get(),
            index_version,
            indexed_graph_version: indexed_graph_version.map(GraphVersion::get),
            stale,
            feedback: Some(FeedbackHandle {
                schema_version: 1,
                trace_id: context.trace_id.clone(),
                request_id: context.request_id.clone(),
            }),
        }
    }
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
