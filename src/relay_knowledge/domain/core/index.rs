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
mod tests {
    use super::*;

    #[test]
    fn empty_index_is_stale_for_newer_graph_version() {
        let status = IndexStatus::empty(IndexKind::Bm25);

        assert!(status.is_stale_for(GraphVersion::new(1)));
    }

    #[test]
    fn index_kind_has_stable_display_values() {
        assert_eq!(IndexKind::Bm25.to_string(), "bm25");
        assert_eq!(IndexKind::Semantic.to_string(), "semantic");
        assert_eq!(IndexKind::Vector.to_string(), "vector");
    }

    #[test]
    fn index_modality_has_stable_display_values() {
        assert_eq!(IndexModality::Text.to_string(), "text");
        assert_eq!(IndexModality::Image.to_string(), "image");
        assert_eq!(IndexModality::Layout.to_string(), "layout");
        assert_eq!(IndexModality::Table.to_string(), "table");
    }

    #[test]
    fn index_state_has_stable_storage_values() {
        assert_eq!(IndexState::Fresh.as_str(), "fresh");
        assert_eq!(IndexState::Stale.as_str(), "stale");
        assert_eq!(IndexState::Failed.as_str(), "failed");
        assert_eq!(IndexState::Paused.as_str(), "paused");
    }
}
