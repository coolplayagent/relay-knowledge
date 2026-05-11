//! Pure domain types.

use serde::{Deserialize, Serialize};

/// A minimal entity model used by early graph-building code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEntity {
    id: String,
    label: String,
}

impl KnowledgeEntity {
    /// Creates a new knowledge entity with a stable identifier and display label.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    /// Returns the stable entity identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the human-readable entity label.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// Monotonic graph state version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GraphVersion(u64);

impl GraphVersion {
    /// The empty graph version used before storage is attached.
    pub const ZERO: Self = Self(0);

    /// Creates a graph version from its numeric representation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric graph version.
    pub const fn get(self) -> u64 {
        self.0
    }
}
