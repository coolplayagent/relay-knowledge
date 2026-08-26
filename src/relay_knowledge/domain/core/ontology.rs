use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{DomainError, SourceScope};

/// Typed ontology node identity. Untyped legacy graph entities keep their label-derived ids.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyEntityKind {
    #[default]
    Untyped,
    BusinessDomain,
    BusinessTerm,
}

impl OntologyEntityKind {
    /// Stable storage and wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Untyped => "untyped",
            Self::BusinessDomain => "business_domain",
            Self::BusinessTerm => "business_term",
        }
    }
}

/// Immutable ontology identity independent of a display label.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OntologyIdentity {
    pub source_scope: SourceScope,
    pub domain_id: String,
    pub entity_id: String,
    pub entity_kind: OntologyEntityKind,
}

impl OntologyIdentity {
    /// Validates the scoped identity used to create stable typed entity ids.
    pub fn new(
        source_scope: SourceScope,
        domain_id: impl Into<String>,
        entity_id: impl Into<String>,
        entity_kind: OntologyEntityKind,
    ) -> Result<Self, DomainError> {
        let domain_id = validate_identity_text("domain_id", domain_id.into())?;
        let entity_id = validate_identity_text("entity_id", entity_id.into())?;
        if entity_kind == OntologyEntityKind::Untyped {
            return Err(DomainError::invalid(
                "entity_kind",
                "scoped ontology identities must be typed",
            ));
        }
        Ok(Self {
            source_scope,
            domain_id,
            entity_id,
            entity_kind,
        })
    }

    /// Returns a deterministic id that does not depend on the display name.
    pub fn stable_entity_id(&self) -> String {
        let mut digest = Sha256::new();
        for part in [
            self.source_scope.as_str(),
            self.domain_id.as_str(),
            self.entity_id.as_str(),
            self.entity_kind.as_str(),
        ] {
            digest.update((part.len() as u64).to_be_bytes());
            digest.update(part.as_bytes());
        }
        format!("ontology:{:x}", digest.finalize())
    }
}

fn validate_identity_text(field: &'static str, value: String) -> Result<String, DomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(DomainError::invalid(field, "must not be empty"));
    }
    if value.len() > 128 {
        return Err(DomainError::invalid(field, "must be 128 bytes or less"));
    }
    if value.contains('\0') {
        return Err(DomainError::invalid(field, "must not contain NUL bytes"));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
#[path = "ontology_tests.rs"]
mod tests;
