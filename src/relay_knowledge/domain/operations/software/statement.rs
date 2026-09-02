use serde::{Deserialize, Serialize};

use super::ontology::{SoftwareEvidenceRef, SoftwareSourceKind};
use super::validation::stable_software_id;
use super::vocabulary::SOFTWARE_PROPERTIES;

/// Controlled relationship vocabulary for provenance-bearing software statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum SoftwarePredicate {
    Contains,
    ProvidesApi,
    ConsumesApi,
    DependsOn,
    Configures,
    Builds,
    Produces,
    Packages,
    Deploys,
    RunsAs,
    Tests,
    Documents,
    DerivedFrom,
    ObservedAs,
    Supersedes,
}

impl SoftwarePredicate {
    pub const fn as_str(self) -> &'static str {
        SOFTWARE_PROPERTIES[self as usize].id
    }

    /// RDF local name declared by the shared OWL object-property vocabulary.
    pub const fn rdf_local_name(self) -> &'static str {
        SOFTWARE_PROPERTIES[self as usize].rdf_local_name
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "contains" => Some(Self::Contains),
            "provides_api" => Some(Self::ProvidesApi),
            "consumes_api" => Some(Self::ConsumesApi),
            "depends_on" => Some(Self::DependsOn),
            "configures" => Some(Self::Configures),
            "builds" => Some(Self::Builds),
            "produces" => Some(Self::Produces),
            "packages" => Some(Self::Packages),
            "deploys" => Some(Self::Deploys),
            "runs_as" => Some(Self::RunsAs),
            "tests" => Some(Self::Tests),
            "documents" => Some(Self::Documents),
            "derived_from" => Some(Self::DerivedFrom),
            "observed_as" => Some(Self::ObservedAs),
            "supersedes" => Some(Self::Supersedes),
            _ => None,
        }
    }
}

/// How a statement entered the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareAssertionMode {
    Declared,
    Extracted,
    Observed,
    Verified,
    Inferred,
}

impl SoftwareAssertionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::Extracted => "extracted",
            Self::Observed => "observed",
            Self::Verified => "verified",
            Self::Inferred => "inferred",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "declared" => Some(Self::Declared),
            "extracted" => Some(Self::Extracted),
            "observed" => Some(Self::Observed),
            "verified" => Some(Self::Verified),
            "inferred" => Some(Self::Inferred),
            _ => None,
        }
    }
}

/// Resolution state is independent from whether the assertion is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareStatementResolution {
    Resolved,
    Unresolved,
    Ambiguous,
    External,
    Conflicting,
}

impl SoftwareStatementResolution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
            Self::Ambiguous => "ambiguous",
            Self::External => "external",
            Self::Conflicting => "conflicting",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "resolved" => Some(Self::Resolved),
            "unresolved" => Some(Self::Unresolved),
            "ambiguous" => Some(Self::Ambiguous),
            "external" => Some(Self::External),
            "conflicting" => Some(Self::Conflicting),
            _ => None,
        }
    }
}

/// Lifecycle state for a statement; conflicting facts remain queryable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareFactState {
    Active,
    Conflicting,
    Superseded,
    Rejected,
}

impl SoftwareFactState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Conflicting => "conflicting",
            Self::Superseded => "superseded",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "conflicting" => Some(Self::Conflicting),
            "superseded" => Some(Self::Superseded),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

/// A first-class assertion retaining provenance, time, extraction, and conflict state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareStatement {
    pub statement_id: String,
    pub subject_id: String,
    pub predicate: SoftwarePredicate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_value: Option<String>,
    pub source_scope: String,
    pub source_kind: SoftwareSourceKind,
    pub evidence_refs: Vec<SoftwareEvidenceRef>,
    pub assertion_mode: SoftwareAssertionMode,
    pub resolution_state: SoftwareStatementResolution,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<u64>,
    pub extractor_id: String,
    pub extractor_version: String,
    pub confidence_basis_points: u16,
    pub fact_state: SoftwareFactState,
}

impl SoftwareStatement {
    /// Builds a candidate identity while leaving shape acceptance to the validator.
    pub fn candidate(input: SoftwareStatementInput) -> Self {
        let subject_id = input.subject_id.trim().to_owned();
        let object_id = normalized_optional(input.object_id);
        let object_value = normalized_optional(input.object_value);
        let source_scope = input.source_scope.trim().to_owned();
        let extractor_id = input.extractor_id.trim().to_owned();
        let extractor_version = input.extractor_version.trim().to_owned();
        let object_identity = object_id
            .as_deref()
            .or(object_value.as_deref())
            .unwrap_or("missing-object");
        let evidence_identity = input
            .evidence_refs
            .iter()
            .map(|reference| reference.evidence_id.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let statement_id = stable_software_id(
            "software_statement",
            [
                subject_id.as_str(),
                input.predicate.as_str(),
                object_identity,
                source_scope.as_str(),
                evidence_identity.as_str(),
                extractor_id.as_str(),
                extractor_version.as_str(),
            ],
        );

        Self {
            statement_id,
            subject_id,
            predicate: input.predicate,
            object_id,
            object_value,
            source_scope,
            source_kind: input.source_kind,
            evidence_refs: input.evidence_refs,
            assertion_mode: input.assertion_mode,
            resolution_state: input.resolution_state,
            valid_from: input.valid_from,
            valid_to: input.valid_to,
            observed_at: input.observed_at,
            extractor_id,
            extractor_version,
            confidence_basis_points: input.confidence_basis_points,
            fact_state: input.fact_state,
        }
    }

    /// Stable comparison key used to retain competing objects without source precedence.
    pub fn object_identity(&self) -> Option<&str> {
        self.object_id.as_deref().or(self.object_value.as_deref())
    }
}

/// Candidate input for `SoftwareStatement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareStatementInput {
    pub subject_id: String,
    pub predicate: SoftwarePredicate,
    pub object_id: Option<String>,
    pub object_value: Option<String>,
    pub source_scope: String,
    pub source_kind: SoftwareSourceKind,
    pub evidence_refs: Vec<SoftwareEvidenceRef>,
    pub assertion_mode: SoftwareAssertionMode,
    pub resolution_state: SoftwareStatementResolution,
    pub valid_from: Option<u64>,
    pub valid_to: Option<u64>,
    pub observed_at: Option<u64>,
    pub extractor_id: String,
    pub extractor_version: String,
    pub confidence_basis_points: u16,
    pub fact_state: SoftwareFactState,
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
