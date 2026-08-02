use serde::{Deserialize, Serialize};

use super::super::{DomainError, GraphVersion, RepositoryCodeRange, error::required_text};
use super::validation::{normalize_optional, stable_software_id, validate_confidence};

/// Projected dependency component from repository manifests and lockfiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareComponent {
    pub component_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub ecosystem: String,
    pub language_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_version: Option<String>,
    pub dependency_group: String,
    pub source_kind: String,
    pub relationship_state: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

impl SoftwareComponent {
    /// Creates a validated component identity derived from dependency evidence.
    pub fn new(input: SoftwareComponentInput) -> Result<Self, DomainError> {
        let requirement = normalize_optional("requirement", input.requirement)?;
        let resolved_version = normalize_optional("resolved_version", input.resolved_version)?;
        let source_scope = required_text("source_scope", input.source_scope)?;
        let ecosystem = required_text("ecosystem", input.ecosystem)?;
        let name = required_text("component_name", input.name)?;
        let dependency_group = required_text("dependency_group", input.dependency_group)?;
        let source_kind = required_text("source_kind", input.source_kind)?;
        let language_id = required_text("language_id", input.language_id)?;
        let evidence_path = required_text("evidence_path", input.evidence_path)?;
        let line_start = input.evidence_line_range.start.to_string();
        let identity_version = resolved_version
            .as_deref()
            .or(requirement.as_deref())
            .unwrap_or("unversioned");

        Ok(Self {
            component_id: stable_software_id(
                "component",
                [
                    source_scope.as_str(),
                    ecosystem.as_str(),
                    name.as_str(),
                    identity_version,
                    dependency_group.as_str(),
                    source_kind.as_str(),
                    language_id.as_str(),
                    evidence_path.as_str(),
                    line_start.as_str(),
                ],
            ),
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            ecosystem,
            name,
            requirement,
            resolved_version,
            dependency_group,
            source_kind,
            relationship_state: required_text("relationship_state", input.relationship_state)?,
            language_id,
            evidence_path,
            evidence_line_range: input.evidence_line_range,
            confidence_basis_points: validate_confidence(input.confidence_basis_points)?,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareComponent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareComponentInput {
    pub repository_id: String,
    pub source_scope: String,
    pub ecosystem: String,
    pub language_id: String,
    pub name: String,
    pub requirement: Option<String>,
    pub resolved_version: Option<String>,
    pub dependency_group: String,
    pub source_kind: String,
    pub relationship_state: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

/// Projected SDK or external API usage from unresolved import/include evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareSdkUsage {
    pub usage_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub language_id: String,
    pub module: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

impl SoftwareSdkUsage {
    /// Creates a validated unresolved SDK/API usage candidate.
    pub fn new(input: SoftwareSdkUsageInput) -> Result<Self, DomainError> {
        let source_scope = required_text("source_scope", input.source_scope)?;
        let language_id = required_text("language_id", input.language_id)?;
        let module = required_text("module", input.module)?;
        let target_hint = normalize_optional("target_hint", input.target_hint)?;
        let resolution_state = required_text("resolution_state", input.resolution_state)?;
        let evidence_path = required_text("evidence_path", input.evidence_path)?;
        let line_start = input.evidence_line_range.start.to_string();

        Ok(Self {
            usage_id: stable_software_id(
                "sdk_usage",
                [
                    source_scope.as_str(),
                    language_id.as_str(),
                    evidence_path.as_str(),
                    module.as_str(),
                    resolution_state.as_str(),
                    line_start.as_str(),
                ],
            ),
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            language_id,
            module,
            target_hint,
            resolution_state,
            evidence_path,
            evidence_line_range: input.evidence_line_range,
            confidence_basis_points: validate_confidence(input.confidence_basis_points)?,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareSdkUsage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareSdkUsageInput {
    pub repository_id: String,
    pub source_scope: String,
    pub language_id: String,
    pub module: String,
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

/// Import/include evidence that uses a declared dependency component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareDependencyUsage {
    pub usage_id: String,
    pub component_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub ecosystem: String,
    pub package_name: String,
    pub language_id: String,
    pub module: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

impl SoftwareDependencyUsage {
    /// Creates a validated relationship between dependency metadata and import evidence.
    pub fn new(input: SoftwareDependencyUsageInput) -> Result<Self, DomainError> {
        let component_id = required_text("component_id", input.component_id)?;
        let source_scope = required_text("source_scope", input.source_scope)?;
        let ecosystem = required_text("ecosystem", input.ecosystem)?;
        let package_name = required_text("package_name", input.package_name)?;
        let language_id = required_text("language_id", input.language_id)?;
        let module = required_text("module", input.module)?;
        let target_hint = normalize_optional("target_hint", input.target_hint)?;
        let resolution_state = required_text("resolution_state", input.resolution_state)?;
        let evidence_path = required_text("evidence_path", input.evidence_path)?;
        let line_start = input.evidence_line_range.start.to_string();

        Ok(Self {
            usage_id: stable_software_id(
                "dependency_usage",
                [
                    source_scope.as_str(),
                    component_id.as_str(),
                    language_id.as_str(),
                    evidence_path.as_str(),
                    module.as_str(),
                    line_start.as_str(),
                ],
            ),
            component_id,
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            ecosystem,
            package_name,
            language_id,
            module,
            target_hint,
            resolution_state,
            evidence_path,
            evidence_line_range: input.evidence_line_range,
            confidence_basis_points: validate_confidence(input.confidence_basis_points)?,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareDependencyUsage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareDependencyUsageInput {
    pub component_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub ecosystem: String,
    pub package_name: String,
    pub language_id: String,
    pub module: String,
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

#[cfg(test)]
#[path = "dependencies_tests.rs"]
mod tests;
