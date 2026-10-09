use serde::{Deserialize, Serialize};

use crate::domain::DomainError;

pub const MAX_CATALOG_SOURCES: usize = 256;
pub const MAX_RESEARCH_JSON_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCatalog {
    pub schema_version: u16,
    pub adapter: String,
    pub sources: Vec<SourceCapture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCapture {
    pub id: String,
    pub url: String,
    pub transport: Option<CaptureTransport>,
    pub raw: Option<ResearchArtifact>,
    pub extraction: Option<CaptureExtraction>,
    pub parent_source: Option<String>,
    #[serde(default)]
    pub references: Vec<ResearchArtifact>,
    #[serde(default)]
    pub expected_sections: Vec<String>,
    pub declared_coverage: Option<CaptureCoverage>,
    pub review: Option<ResearchReviewClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTransport {
    pub http_status: Option<u16>,
    pub final_url: Option<String>,
    #[serde(default)]
    pub redirects: Vec<String>,
    pub access_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchPathBase {
    Repository,
    Catalog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchArtifact {
    pub path_base: ResearchPathBase,
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureExtraction {
    pub artifact: ResearchArtifact,
    pub raw_sha256: String,
    pub extractor: String,
    pub extractor_version: String,
    pub expected_extractor: Option<String>,
    pub expected_extractor_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureCoverage {
    Shell,
    FullBody,
    Unknown,
}

/// Locally supplied review claims are retained with their origin, never authenticated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchReviewClaim {
    pub content_sha256: String,
    pub reviewer: String,
    pub origin: String,
    pub event_id: Option<String>,
}

impl SourceCatalog {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != 1 || self.adapter != "relay-capture-v1" {
            return Err(DomainError::invalid(
                "catalog",
                "requires schema_version 1 and explicit relay-capture-v1 adapter",
            ));
        }
        if self.sources.is_empty() || self.sources.len() > MAX_CATALOG_SOURCES {
            return Err(DomainError::invalid("sources", "requires 1..256 captures"));
        }
        if serde_json::to_vec(self)
            .map_err(|error| DomainError::invalid("catalog", error.to_string()))?
            .len()
            > MAX_RESEARCH_JSON_BYTES
        {
            return Err(DomainError::invalid("catalog", "exceeds 2 MiB"));
        }
        for source in &self.sources {
            bounded_text(&source.id, "source.id", 128)?;
            bounded_text(&source.url, "source.url", 4096)?;
            if source.references.len() > 32 || source.expected_sections.len() > 64 {
                return Err(DomainError::invalid(
                    "source",
                    "at most 32 references and 64 expected sections",
                ));
            }
            for section in &source.expected_sections {
                bounded_text(section, "expected_sections", 512)?;
            }
            if let Some(transport) = &source.transport {
                if transport.redirects.len() > 16
                    || transport
                        .http_status
                        .is_some_and(|code| !(100..=599).contains(&code))
                {
                    return Err(DomainError::invalid(
                        "transport",
                        "invalid HTTP status or more than 16 redirects",
                    ));
                }
            }
            for artifact in source
                .raw
                .iter()
                .chain(source.extraction.iter().map(|value| &value.artifact))
                .chain(source.references.iter())
            {
                artifact.validate()?;
            }
            if let Some(extraction) = &source.extraction {
                validate_sha256(&extraction.raw_sha256)?;
                bounded_text(&extraction.extractor, "extractor", 256)?;
                bounded_text(&extraction.extractor_version, "extractor_version", 256)?;
            }
            if let Some(review) = &source.review {
                validate_sha256(&review.content_sha256)?;
                bounded_text(&review.reviewer, "reviewer", 256)?;
                bounded_text(&review.origin, "review.origin", 256)?;
            }
        }
        Ok(())
    }
}

impl ResearchArtifact {
    pub fn validate(&self) -> Result<(), DomainError> {
        bounded_text(&self.path, "artifact.path", 4096)?;
        validate_sha256(&self.sha256)
    }
}

pub(crate) fn bounded_text(
    value: &str,
    field: &'static str,
    max: usize,
) -> Result<(), DomainError> {
    if value.trim().is_empty() || value.len() > max {
        return Err(DomainError::invalid(
            field,
            format!("must contain 1..{max} bytes"),
        ));
    }
    Ok(())
}

pub fn validate_sha256(value: &str) -> Result<(), DomainError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(DomainError::invalid(
            "sha256",
            "requires a lowercase SHA-256 digest",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
