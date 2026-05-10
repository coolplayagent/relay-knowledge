//! Core primitives for the relay-knowledge knowledge graph.

/// A minimal entity model used by early graph-building code.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Returns the crate's canonical project name.
pub fn project_name() -> &'static str {
    "relay-knowledge"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_entity_with_id_and_label() {
        let entity = KnowledgeEntity::new("entity:rust", "Rust");

        assert_eq!(entity.id(), "entity:rust");
        assert_eq!(entity.label(), "Rust");
    }
}
