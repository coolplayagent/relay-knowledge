//! Core primitives and API boundaries for the relay-knowledge knowledge graph.

pub mod api;
pub mod application;
pub mod domain;
pub mod interfaces;

pub use domain::KnowledgeEntity;

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
