use serde::{Deserialize, Serialize};

use super::super::{DomainError, GraphVersion, RepositoryCodeRange, error::required_text};
use super::validation::{normalize_optional, stable_software_id, validate_confidence};

/// Build target, script, profile, or generator entry projected from indexed repository evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareBuildTarget {
    pub target_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub ecosystem: String,
    pub language_id: String,
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_hint: Option<String>,
    pub source_kind: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

impl SoftwareBuildTarget {
    /// Creates a validated build target projection row.
    pub fn new(input: SoftwareBuildTargetInput) -> Result<Self, DomainError> {
        let source_scope = required_text("source_scope", input.source_scope)?;
        let ecosystem = required_text("ecosystem", input.ecosystem)?;
        let language_id = required_text("language_id", input.language_id)?;
        let name = required_text("build_target_name", input.name)?;
        let kind = required_text("build_target_kind", input.kind)?;
        let command = normalize_optional("command", input.command)?;
        let output_hint = normalize_optional("output_hint", input.output_hint)?;
        let source_kind = required_text("source_kind", input.source_kind)?;
        let evidence_path = required_text("evidence_path", input.evidence_path)?;
        let line_start = input.evidence_line_range.start.to_string();

        Ok(Self {
            target_id: stable_software_id(
                "build_target",
                [
                    source_scope.as_str(),
                    ecosystem.as_str(),
                    language_id.as_str(),
                    name.as_str(),
                    kind.as_str(),
                    source_kind.as_str(),
                    evidence_path.as_str(),
                    line_start.as_str(),
                ],
            ),
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            ecosystem,
            language_id,
            name,
            kind,
            command,
            output_hint,
            source_kind,
            evidence_path,
            evidence_line_range: input.evidence_line_range,
            confidence_basis_points: validate_confidence(input.confidence_basis_points)?,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareBuildTarget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareBuildTargetInput {
    pub repository_id: String,
    pub source_scope: String,
    pub ecosystem: String,
    pub language_id: String,
    pub name: String,
    pub kind: String,
    pub command: Option<String>,
    pub output_hint: Option<String>,
    pub source_kind: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

/// Infrastructure, deployment, or service-operation resource projected from indexed IaC evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareIacResource {
    pub resource_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub language_id: String,
    pub provider: String,
    pub resource_kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub source_kind: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

impl SoftwareIacResource {
    /// Creates a validated IaC/deployment resource projection row.
    pub fn new(input: SoftwareIacResourceInput) -> Result<Self, DomainError> {
        let source_scope = required_text("source_scope", input.source_scope)?;
        let language_id = required_text("language_id", input.language_id)?;
        let provider = required_text("iac_provider", input.provider)?;
        let resource_kind = required_text("iac_resource_kind", input.resource_kind)?;
        let name = required_text("iac_resource_name", input.name)?;
        let scope_hint = normalize_optional("scope_hint", input.scope_hint)?;
        let target_hint = normalize_optional("target_hint", input.target_hint)?;
        let resolution_state = required_text("resolution_state", input.resolution_state)?;
        let source_kind = required_text("source_kind", input.source_kind)?;
        let evidence_path = required_text("evidence_path", input.evidence_path)?;
        let line_start = input.evidence_line_range.start.to_string();

        Ok(Self {
            resource_id: stable_software_id(
                "iac_resource",
                [
                    source_scope.as_str(),
                    language_id.as_str(),
                    provider.as_str(),
                    resource_kind.as_str(),
                    name.as_str(),
                    source_kind.as_str(),
                    evidence_path.as_str(),
                    line_start.as_str(),
                ],
            ),
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            language_id,
            provider,
            resource_kind,
            name,
            scope_hint,
            target_hint,
            resolution_state,
            source_kind,
            evidence_path,
            evidence_line_range: input.evidence_line_range,
            confidence_basis_points: validate_confidence(input.confidence_basis_points)?,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareIacResource`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareIacResourceInput {
    pub repository_id: String,
    pub source_scope: String,
    pub language_id: String,
    pub provider: String,
    pub resource_kind: String,
    pub name: String,
    pub scope_hint: Option<String>,
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub source_kind: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

/// Design, architecture, module, component, interface, or capability evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareDesignElement {
    pub element_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub language_id: String,
    pub element_kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub source_kind: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

impl SoftwareDesignElement {
    /// Creates a validated design projection row.
    pub fn new(input: SoftwareDesignElementInput) -> Result<Self, DomainError> {
        let source_scope = required_text("source_scope", input.source_scope)?;
        let language_id = required_text("language_id", input.language_id)?;
        let element_kind = required_text("design_element_kind", input.element_kind)?;
        let name = required_text("design_element_name", input.name)?;
        let parent = normalize_optional("parent", input.parent)?;
        let summary = normalize_optional("summary", input.summary)?;
        let source_kind = required_text("source_kind", input.source_kind)?;
        let evidence_path = required_text("evidence_path", input.evidence_path)?;
        let line_start = input.evidence_line_range.start.to_string();

        Ok(Self {
            element_id: stable_software_id(
                "design_element",
                [
                    source_scope.as_str(),
                    language_id.as_str(),
                    element_kind.as_str(),
                    name.as_str(),
                    source_kind.as_str(),
                    evidence_path.as_str(),
                    line_start.as_str(),
                ],
            ),
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            language_id,
            element_kind,
            name,
            parent,
            summary,
            source_kind,
            evidence_path,
            evidence_line_range: input.evidence_line_range,
            confidence_basis_points: validate_confidence(input.confidence_basis_points)?,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareDesignElement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareDesignElementInput {
    pub repository_id: String,
    pub source_scope: String,
    pub language_id: String,
    pub element_kind: String,
    pub name: String,
    pub parent: Option<String>,
    pub summary: Option<String>,
    pub source_kind: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub confidence_basis_points: u16,
    pub created_graph_version: GraphVersion,
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
