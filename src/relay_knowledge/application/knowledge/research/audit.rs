//! Local capture verification; no network requests or approval inference.

use super::reader::{ResearchReader, digest};
use crate::{
    api::ApiError,
    domain::research::{CaptureCoverage, ResearchArtifact, SourceCapture, SourceCatalog},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResearchDiagnostic {
    pub target: String,
    pub code: String,
    pub message: String,
    pub next_step: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactAudit {
    pub state: String,
    pub path: String,
    pub path_base: crate::domain::research::ResearchPathBase,
    pub expected_sha256: String,
    pub observed_sha256: Option<String>,
    pub byte_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureAudit {
    pub id: String,
    pub url: String,
    pub transport: Option<crate::domain::research::CaptureTransport>,
    pub transport_state: String,
    pub local_capture: Option<ArtifactAudit>,
    pub extraction: Option<ArtifactAudit>,
    pub extraction_state: String,
    pub coverage: String,
    pub declared_coverage: Option<CaptureCoverage>,
    pub review: String,
    pub review_claim: Option<crate::domain::research::ResearchReviewClaim>,
    pub index_freshness: String,
    pub references: Vec<ArtifactAudit>,
    pub diagnostics: Vec<ResearchDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAuditReport {
    pub schema_version: u16,
    pub catalog_sha256: String,
    pub integrity_valid: bool,
    pub captures: Vec<CaptureAudit>,
    pub identical_byte_groups: Vec<Vec<String>>,
    pub diagnostics: Vec<ResearchDiagnostic>,
}

pub(super) fn audit_catalog(
    reader: &mut ResearchReader,
    input: &Path,
) -> Result<SourceAuditReport, ApiError> {
    let (catalog, catalog_sha256): (SourceCatalog, _) = reader.json(input)?;
    catalog
        .validate()
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let mut diagnostics = Vec::new();
    let mut identities = BTreeSet::new();
    for source in &catalog.sources {
        if !identities.insert(&source.id) {
            diagnostics.push(diagnostic(
                &source.id,
                "duplicate_id",
                "catalog source ids must be unique",
                "Assign distinct stable ids; preserve the original URL on each capture.",
            ));
        }
        let mut seen = BTreeSet::new();
        let mut current = source;
        while let Some(parent) = &current.parent_source {
            if !seen.insert(parent) {
                diagnostics.push(diagnostic(
                    &source.id,
                    "source_chain_cycle",
                    "capture lineage contains a cycle",
                    "Correct the parent source references.",
                ));
                break;
            }
            let Some(next) = catalog
                .sources
                .iter()
                .find(|candidate| &candidate.id == parent)
            else {
                diagnostics.push(diagnostic(
                    &source.id,
                    "missing_parent_source",
                    "capture lineage points to an absent source",
                    "Add the referenced parent capture or correct its id.",
                ));
                break;
            };
            current = next;
        }
    }
    let captures: Vec<_> = catalog
        .sources
        .iter()
        .map(|source| audit_capture(reader, input, source))
        .collect();
    let mut hashes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for capture in &captures {
        if let Some(hash) = capture
            .local_capture
            .as_ref()
            .and_then(|raw| raw.observed_sha256.as_ref())
        {
            hashes
                .entry(hash.clone())
                .or_default()
                .push(capture.id.clone());
        }
    }
    Ok(SourceAuditReport {
        schema_version: 1,
        catalog_sha256,
        integrity_valid: diagnostics.is_empty()
            && captures
                .iter()
                .all(|capture| capture.diagnostics.is_empty()),
        captures,
        identical_byte_groups: hashes.into_values().filter(|ids| ids.len() > 1).collect(),
        diagnostics,
    })
}

fn audit_capture(
    reader: &mut ResearchReader,
    input: &Path,
    source: &SourceCapture,
) -> CaptureAudit {
    let mut result = CaptureAudit {
        id: source.id.clone(),
        url: source.url.clone(),
        transport: source.transport.clone(),
        transport_state: match source.transport.as_ref() {
            None => "unknown",
            Some(value)
                if value.access_error.is_some()
                    || value.http_status.is_some_and(|code| code >= 400) =>
            {
                "reported_access_failure"
            }
            Some(_) => "reported_only",
        }
        .into(),
        local_capture: None,
        extraction: None,
        extraction_state: "not_provided".into(),
        coverage: "unknown".into(),
        declared_coverage: source.declared_coverage.clone(),
        review: "unknown".into(),
        review_claim: source.review.clone(),
        index_freshness: "not_assessed".into(),
        references: Vec::new(),
        diagnostics: Vec::new(),
    };
    if let Some(raw) = &source.raw {
        result.local_capture =
            Some(audit_artifact(reader, input, raw, &source.id, &mut result.diagnostics).0);
    } else {
        result.diagnostics.push(diagnostic(&source.id, "missing_capture", "no local raw artifact is declared", "Record a local capture and its exact byte hash; access status alone is not an archive."));
    }
    let mut text = None;
    if let Some(extraction) = &source.extraction {
        let (audit, bytes) = audit_artifact(
            reader,
            input,
            &extraction.artifact,
            &source.id,
            &mut result.diagnostics,
        );
        result.extraction_state = audit.state.clone();
        result.extraction = Some(audit);
        if result
            .local_capture
            .as_ref()
            .and_then(|raw| raw.observed_sha256.as_deref())
            != Some(&extraction.raw_sha256)
        {
            result.extraction_state = "stale_source".into();
            result.diagnostics.push(diagnostic(
                &source.id,
                "extraction_source_changed",
                "extraction is not bound to the observed raw bytes",
                "Re-extract from the current raw artifact and record its hash.",
            ));
        }
        if extraction
            .expected_extractor
            .as_ref()
            .is_some_and(|expected| expected != &extraction.extractor)
            || extraction
                .expected_extractor_version
                .as_ref()
                .is_some_and(|expected| expected != &extraction.extractor_version)
        {
            result.extraction_state = "extractor_changed".into();
            result.diagnostics.push(diagnostic(
                &source.id,
                "extractor_changed",
                "recorded extractor does not match the declared expectation",
                "Review the extraction tool/version change and regenerate evidence when required.",
            ));
        }
        if let Some(bytes) = bytes {
            match String::from_utf8(bytes) {
                Ok(value) => text = Some(value),
                Err(_) => {
                    result.extraction_state = "invalid_text".into();
                    result.diagnostics.push(diagnostic(
                        &source.id,
                        "invalid_extracted_text",
                        "extracted text is not UTF-8",
                        "Declare a UTF-8 extraction artifact without modifying the raw original.",
                    ));
                }
            }
        }
    }
    result.coverage = coverage(source, text.as_deref(), &result.extraction_state);
    if let Some(review) = &source.review {
        let observed = result
            .extraction
            .as_ref()
            .or(result.local_capture.as_ref())
            .and_then(|artifact| artifact.observed_sha256.as_deref());
        result.review = if observed == Some(&review.content_sha256) {
            "self_reported_unverified"
        } else {
            "needs_review"
        }
        .into();
    }
    for reference in &source.references {
        result.references.push(
            audit_artifact(
                reader,
                input,
                reference,
                &source.id,
                &mut result.diagnostics,
            )
            .0,
        );
    }
    if source.review.is_some() && !result.diagnostics.is_empty() {
        result.review = "needs_review".into();
    }
    result
}

fn coverage(source: &SourceCapture, text: Option<&str>, extraction_state: &str) -> String {
    if source.declared_coverage == Some(CaptureCoverage::Shell) {
        return "declared_shell".into();
    }
    if source.expected_sections.is_empty() {
        return "unknown".into();
    }
    if extraction_state != "verified" {
        return "unverifiable".into();
    }
    let Some(text) = text else {
        return "unverifiable".into();
    };
    if source.expected_sections.iter().all(|section| {
        text.lines()
            .any(|line| line.trim().trim_start_matches('#').trim() == section.trim())
    }) {
        "declared_sections_present".into()
    } else {
        "missing_declared_sections".into()
    }
}

pub(super) fn audit_artifact(
    reader: &mut ResearchReader,
    input: &Path,
    artifact: &ResearchArtifact,
    target: &str,
    diagnostics: &mut Vec<ResearchDiagnostic>,
) -> (ArtifactAudit, Option<Vec<u8>>) {
    let mut result = ArtifactAudit {
        state: "unreadable".into(),
        path: artifact.path.clone(),
        path_base: artifact.path_base.clone(),
        expected_sha256: artifact.sha256.clone(),
        observed_sha256: None,
        byte_count: None,
    };
    match reader.artifact(artifact, input) {
        Ok(bytes) => {
            let observed = digest(&bytes);
            result.state = if observed == artifact.sha256 {
                "verified"
            } else {
                "hash_mismatch"
            }
            .into();
            if observed != artifact.sha256 {
                diagnostics.push(diagnostic(target, "hash_mismatch", &format!("{} differs from its declared hash", artifact.path), "Recover the original or explicitly rebind derived evidence to the changed source; do not rewrite raw bytes to satisfy formatting."));
            }
            result.observed_sha256 = Some(observed);
            result.byte_count = Some(bytes.len());
            (result, Some(bytes))
        }
        Err(error) => {
            diagnostics.push(diagnostic(
                target,
                "artifact_unreadable",
                &format!("{}: {error}", artifact.path),
                "Check the declared path base, file existence, scope, permissions and read budget.",
            ));
            (result, None)
        }
    }
}

pub(super) fn diagnostic(
    target: &str,
    code: &str,
    message: &str,
    next_step: &str,
) -> ResearchDiagnostic {
    ResearchDiagnostic {
        target: target.into(),
        code: code.into(),
        message: message.into(),
        next_step: next_step.into(),
    }
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
