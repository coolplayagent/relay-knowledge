use std::fmt;

use serde::{Deserialize, Serialize};

use super::GraphVersion;

/// Derived index families maintained from the graph mutation log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexKind {
    Bm25,
    Semantic,
    Vector,
}

impl IndexKind {
    /// All v1 index families required by the hybrid retrieval contract.
    pub const ALL: [Self; 3] = [Self::Bm25, Self::Semantic, Self::Vector];

    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bm25 => "bm25",
            Self::Semantic => "semantic",
            Self::Vector => "vector",
        }
    }
}

impl fmt::Display for IndexKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Source modality covered by a derived index cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexModality {
    Text,
    Image,
    Layout,
    Table,
}

impl IndexModality {
    /// The v1 evidence modality refreshed by BM25, semantic, and vector indexes.
    pub const TEXT: Self = Self::Text;

    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Layout => "layout",
            Self::Table => "table",
        }
    }
}

impl fmt::Display for IndexModality {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Operational state of a derived index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    Fresh,
    Stale,
    Failed,
    Paused,
}

impl IndexState {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Failed => "failed",
            Self::Paused => "paused",
        }
    }
}

/// Versioned status for a derived index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStatus {
    pub kind: IndexKind,
    pub index_version: u64,
    pub indexed_graph_version: GraphVersion,
    pub state: IndexState,
    pub last_error: Option<String>,
}

/// Scoped cursor for a derived index read model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexCursor {
    pub kind: IndexKind,
    pub source_scope: String,
    pub modality: IndexModality,
    pub index_version: u64,
    pub indexed_graph_version: GraphVersion,
    pub state: IndexState,
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_dimension: Option<u32>,
}

/// Per-kind lag included in diagnostics snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexLag {
    pub kind: IndexKind,
    pub lag_versions: u64,
}

/// Structured reason explaining why an index family or scoped cursor is stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStalenessReason {
    pub kind: IndexKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modality: Option<IndexModality>,
    pub reason: String,
    pub lag_versions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Queue, dead-letter, and stale-index diagnostics shared by APIs and storage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRefreshDiagnostics {
    pub queue_depth: usize,
    pub running_count: usize,
    pub retrying_count: usize,
    pub dead_letter_count: usize,
    pub oldest_unfinished_age_ms: Option<u64>,
    pub index_lag_by_kind: Vec<IndexLag>,
    pub max_index_lag_versions: u64,
    pub stale_index_count: usize,
    pub stale_reasons: Vec<IndexStalenessReason>,
}

impl IndexStatus {
    /// Creates the initial stale status for an empty derived index.
    pub const fn empty(kind: IndexKind) -> Self {
        Self {
            kind,
            index_version: 0,
            indexed_graph_version: GraphVersion::ZERO,
            state: IndexState::Stale,
            last_error: None,
        }
    }

    /// Returns whether this index is behind the supplied graph version.
    pub fn is_stale_for(&self, graph_version: GraphVersion) -> bool {
        self.state != IndexState::Fresh || self.indexed_graph_version < graph_version
    }
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
