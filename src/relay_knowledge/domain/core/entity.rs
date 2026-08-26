use serde::{Deserialize, Serialize};

use super::{DomainError, OntologyEntityKind, OntologyIdentity};

/// A minimal entity model used by early graph-building code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEntity {
    id: String,
    label: String,
    #[serde(default)]
    entity_kind: OntologyEntityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ontology_identity: Option<OntologyIdentity>,
}

impl KnowledgeEntity {
    /// Creates a new knowledge entity with a stable identifier and display label.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            entity_kind: OntologyEntityKind::Untyped,
            ontology_identity: None,
        }
    }

    /// Creates a typed entity whose id is derived from scoped ontology identity, not its label.
    pub fn from_ontology(
        identity: OntologyIdentity,
        label: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let label = label.into();
        if label.trim().is_empty() {
            return Err(DomainError::invalid("label", "must not be empty"));
        }
        Ok(Self {
            id: identity.stable_entity_id(),
            label,
            entity_kind: identity.entity_kind,
            ontology_identity: Some(identity),
        })
    }

    /// Returns the stable entity identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the human-readable entity label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns `untyped` for legacy label-only entities and the ontology type otherwise.
    pub const fn entity_kind(&self) -> OntologyEntityKind {
        self.entity_kind
    }

    /// Returns scoped ontology identity when this is a typed entity.
    pub fn ontology_identity(&self) -> Option<&OntologyIdentity> {
        self.ontology_identity.as_ref()
    }
}

#[cfg(test)]
#[path = "entity_tests.rs"]
mod tests;
