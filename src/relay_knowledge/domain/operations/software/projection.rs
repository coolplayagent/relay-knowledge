use serde::{Deserialize, Serialize};

use super::super::GraphVersion;
use super::{
    SoftwareBuildTarget, SoftwareComponent, SoftwareDependencyUsage, SoftwareDesignElement,
    SoftwareEntity, SoftwareFile, SoftwareIacResource, SoftwareRelationship, SoftwareSdkUsage,
    SoftwareShapeDiagnostic, SoftwareSourceKind, SoftwareStatement, SoftwareTopic,
};

/// Current SQLite read-model contract. Older scopes are rebuilt through the durable projection task.
pub const SOFTWARE_PROJECTION_SCHEMA_VERSION: u32 = 6;

/// Read-model freshness kept explicit even when repository scope metadata is unavailable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareProjectionFreshness {
    Fresh,
    #[default]
    Stale,
    Degraded,
}

impl SoftwareProjectionFreshness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Degraded => "degraded",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fresh" => Some(Self::Fresh),
            "stale" => Some(Self::Stale),
            "degraded" => Some(Self::Degraded),
            _ => None,
        }
    }
}

/// Bounded provenance coverage summary for the current source scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareSourceCoverage {
    pub source_kinds: Vec<SoftwareSourceKind>,
    pub source_path_count: usize,
    pub evidence_ref_count: usize,
}

/// Freshness and count summary for the software global projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalStatus {
    pub repository_id: String,
    pub source_scope: String,
    pub projected_graph_version: GraphVersion,
    pub stale: bool,
    #[serde(default = "legacy_ontology_version")]
    pub ontology_version: String,
    #[serde(default)]
    pub projection_schema_version: u32,
    #[serde(default)]
    pub source_coverage: SoftwareSourceCoverage,
    #[serde(default)]
    pub completeness_basis_points: u16,
    #[serde(default)]
    pub freshness: SoftwareProjectionFreshness,
    #[serde(default)]
    pub conflict_count: usize,
    #[serde(default)]
    pub entity_count: usize,
    #[serde(default)]
    pub statement_count: usize,
    #[serde(default)]
    pub diagnostic_count: usize,
    pub component_count: usize,
    pub sdk_usage_count: usize,
    pub file_count: usize,
    pub topic_count: usize,
    pub relationship_count: usize,
    pub build_target_count: usize,
    pub iac_resource_count: usize,
    pub design_element_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

fn legacy_ontology_version() -> String {
    "legacy-unknown".to_owned()
}

/// Projected software global facts for one repository scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalProjection {
    pub status: SoftwareGlobalStatus,
    pub components: Vec<SoftwareComponent>,
    pub dependency_usages: Vec<SoftwareDependencyUsage>,
    pub sdk_usages: Vec<SoftwareSdkUsage>,
    pub files: Vec<SoftwareFile>,
    pub topics: Vec<SoftwareTopic>,
    pub relationships: Vec<SoftwareRelationship>,
    pub build_targets: Vec<SoftwareBuildTarget>,
    pub iac_resources: Vec<SoftwareIacResource>,
    pub design_elements: Vec<SoftwareDesignElement>,
    pub entities: Vec<SoftwareEntity>,
    pub statements: Vec<SoftwareStatement>,
    pub diagnostics: Vec<SoftwareShapeDiagnostic>,
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
