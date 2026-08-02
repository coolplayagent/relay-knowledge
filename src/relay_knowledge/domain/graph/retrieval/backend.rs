use serde::{Deserialize, Serialize};

use super::super::GraphVersion;

/// Retrieval source that contributed to a fused context result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrieverSource {
    Bm25,
    GraphEvidence,
    CodeGraph,
    Semantic,
    Vector,
    GraphPath,
    Temporal,
    CommunitySummary,
}

impl RetrieverSource {
    /// Stable API representation used in ranking diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bm25 => "bm25",
            Self::GraphEvidence => "graph_evidence",
            Self::CodeGraph => "code_graph",
            Self::Semantic => "semantic",
            Self::Vector => "vector",
            Self::GraphPath => "graph_path",
            Self::Temporal => "temporal",
            Self::CommunitySummary => "community_summary",
        }
    }
}

/// Availability state for optional retrieval backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalBackendState {
    Available,
    Degraded,
    Unavailable,
}

/// Per-backend status preserved so callers can distinguish fallback from
/// complete hybrid retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalBackendStatus {
    pub source: RetrieverSource,
    pub state: RetrievalBackendState,
    pub scope_post_filter: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_graph_version: Option<GraphVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
