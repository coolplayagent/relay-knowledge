//! Caller-authored completion criteria remain explicit claims bound to evidence bytes.
use super::{ResearchArtifact, ResearchReviewClaim, catalog::bounded_text};
use crate::domain::DomainError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchDelivery {
    Archive,
    AuthoredGraph,
    Graphrag,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchRequirements {
    pub schema_version: u16,
    pub requirements: Vec<ResearchRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchRequirement {
    pub id: String,
    pub description: String,
    pub evidence: Vec<ResearchArtifact>,
    pub review_claim: Option<ResearchReviewClaim>,
}

impl ResearchRequirements {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != 1 || self.requirements.is_empty() || self.requirements.len() > 100
        {
            return Err(DomainError::invalid(
                "requirements",
                "requires schema version 1 and 1..100 criteria",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for requirement in &self.requirements {
            bounded_text(&requirement.id, "requirement.id", 128)?;
            bounded_text(&requirement.description, "requirement.description", 4096)?;
            if !ids.insert(&requirement.id)
                || requirement.evidence.is_empty()
                || requirement.evidence.len() > 32
            {
                return Err(DomainError::invalid(
                    "requirement",
                    "requires a unique id and 1..32 evidence bindings",
                ));
            }
            if let Some(review) = &requirement.review_claim {
                super::validate_sha256(&review.content_sha256)?;
                bounded_text(&review.reviewer, "reviewer", 256)?;
                bounded_text(&review.origin, "review.origin", 256)?;
            }
            for artifact in &requirement.evidence {
                artifact.validate()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "requirements_tests.rs"]
mod tests;
