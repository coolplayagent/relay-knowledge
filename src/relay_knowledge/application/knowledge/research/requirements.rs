//! Hash verification is not an automated judgment that a requirement is satisfied.
use super::{
    audit::{ArtifactAudit, ResearchDiagnostic, audit_artifact},
    reader::ResearchReader,
};
use crate::{
    api::ApiError,
    domain::research::{ResearchRequirements, ResearchReviewClaim},
};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct RequirementAudit {
    pub manifest_sha256: String,
    pub content_verdict: String,
    pub requirements: Vec<RequirementEvidenceAudit>,
}

#[derive(Debug, Serialize)]
pub struct RequirementEvidenceAudit {
    pub id: String,
    pub description: String,
    pub evidence_integrity: String,
    pub content_verdict: String,
    pub review_state: String,
    pub review_subject_sha256: String,
    pub review_claim: Option<ResearchReviewClaim>,
    pub artifacts: Vec<ArtifactAudit>,
    pub diagnostics: Vec<ResearchDiagnostic>,
}

pub(super) fn audit_requirements(
    reader: &mut ResearchReader,
    input: &Path,
) -> Result<RequirementAudit, ApiError> {
    let (manifest, manifest_sha256): (ResearchRequirements, _) = reader.json(input)?;
    manifest
        .validate()
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let mut requirements = Vec::new();
    for requirement in manifest.requirements {
        let subject = serde_json::to_vec(&(
            &requirement.id,
            &requirement.description,
            &requirement.evidence,
        ))
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let review_subject_sha256 = super::reader::digest(&subject);
        let mut diagnostics = Vec::new();
        let artifacts = requirement
            .evidence
            .iter()
            .map(|artifact| {
                audit_artifact(reader, input, artifact, &requirement.id, &mut diagnostics).0
            })
            .collect();
        requirements.push(RequirementEvidenceAudit {
            id: requirement.id,
            description: requirement.description,
            evidence_integrity: if diagnostics.is_empty() {
                "verified"
            } else {
                "invalid"
            }
            .into(),
            content_verdict: "unknown".into(),
            review_state: if requirement.review_claim.is_none() {
                "unknown"
            } else if diagnostics.is_empty()
                && requirement
                    .review_claim
                    .as_ref()
                    .is_some_and(|claim| claim.content_sha256 == review_subject_sha256)
            {
                "self_reported_unverified"
            } else {
                "needs_review"
            }
            .into(),
            review_subject_sha256,
            review_claim: requirement.review_claim,
            artifacts,
            diagnostics,
        });
    }
    Ok(RequirementAudit {
        manifest_sha256,
        content_verdict: "unknown".into(),
        requirements,
    })
}
