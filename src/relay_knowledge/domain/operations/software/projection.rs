use serde::{Deserialize, Serialize};

use super::super::GraphVersion;
use super::{
    SoftwareBuildTarget, SoftwareComponent, SoftwareDependencyUsage, SoftwareDesignElement,
    SoftwareFile, SoftwareIacResource, SoftwareRelationship, SoftwareSdkUsage, SoftwareTopic,
};

/// Freshness and count summary for the software global projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalStatus {
    pub repository_id: String,
    pub source_scope: String,
    pub projected_graph_version: GraphVersion,
    pub stale: bool,
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
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
