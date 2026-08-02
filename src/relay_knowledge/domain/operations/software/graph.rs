use serde::{Deserialize, Serialize};

use super::super::{DomainError, GraphVersion, RepositoryCodeRange, error::required_text};
use super::validation::{normalize_optional, stable_software_id, validate_confidence};

/// Projected repository file node for the software global model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareFile {
    pub software_file_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub path: String,
    pub language_id: String,
    pub file_role: String,
    pub parse_status: String,
    pub created_graph_version: GraphVersion,
}

impl SoftwareFile {
    /// Creates a stable file node identity for a repository snapshot.
    pub fn new(input: SoftwareFileInput) -> Result<Self, DomainError> {
        let source_scope = required_text("source_scope", input.source_scope)?;
        let path = required_text("software_file_path", input.path)?;

        Ok(Self {
            software_file_id: stable_software_id("file", [source_scope.as_str(), path.as_str()]),
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            path,
            language_id: required_text("language_id", input.language_id)?,
            file_role: required_text("file_role", input.file_role)?,
            parse_status: required_text("parse_status", input.parse_status)?,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareFile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareFileInput {
    pub repository_id: String,
    pub source_scope: String,
    pub path: String,
    pub language_id: String,
    pub file_role: String,
    pub parse_status: String,
    pub created_graph_version: GraphVersion,
}

/// Topic extracted from repository documentation or the repository knowledge map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareTopic {
    pub topic_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub name: String,
    pub topic_kind: String,
    pub source_path: String,
    pub line_range: RepositoryCodeRange,
    pub created_graph_version: GraphVersion,
}

impl SoftwareTopic {
    /// Creates a stable topic identity tied to the source evidence location.
    pub fn new(input: SoftwareTopicInput) -> Result<Self, DomainError> {
        let source_scope = required_text("source_scope", input.source_scope)?;
        let name = required_text("topic_name", input.name)?;
        let topic_kind = required_text("topic_kind", input.topic_kind)?;
        let source_path = required_text("topic_source_path", input.source_path)?;
        let line_start = input.line_range.start.to_string();

        Ok(Self {
            topic_id: stable_software_id(
                "topic",
                [
                    source_scope.as_str(),
                    topic_kind.as_str(),
                    source_path.as_str(),
                    name.as_str(),
                    line_start.as_str(),
                ],
            ),
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            name,
            topic_kind,
            source_path,
            line_range: input.line_range,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareTopic`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareTopicInput {
    pub repository_id: String,
    pub source_scope: String,
    pub name: String,
    pub topic_kind: String,
    pub source_path: String,
    pub line_range: RepositoryCodeRange,
    pub created_graph_version: GraphVersion,
}

/// Cross-domain relationship between projected software files, topics, components, and usages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareRelationship {
    pub relationship_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub relationship_kind: String,
    pub source_id: String,
    pub source_kind: String,
    pub target_id: String,
    pub target_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub created_graph_version: GraphVersion,
}

impl SoftwareRelationship {
    /// Creates a validated projected relationship without upgrading unresolved targets.
    pub fn new(input: SoftwareRelationshipInput) -> Result<Self, DomainError> {
        let source_scope = required_text("source_scope", input.source_scope)?;
        let relationship_kind = required_text("relationship_kind", input.relationship_kind)?;
        let source_id = required_text("relationship_source_id", input.source_id)?;
        let target_id = required_text("relationship_target_id", input.target_id)?;
        let evidence_path = required_text("evidence_path", input.evidence_path)?;
        let line_start = input.evidence_line_range.start.to_string();

        Ok(Self {
            relationship_id: stable_software_id(
                "relationship",
                [
                    source_scope.as_str(),
                    relationship_kind.as_str(),
                    source_id.as_str(),
                    target_id.as_str(),
                    evidence_path.as_str(),
                    line_start.as_str(),
                ],
            ),
            repository_id: required_text("repository_id", input.repository_id)?,
            source_scope,
            relationship_kind,
            source_id,
            source_kind: required_text("relationship_source_kind", input.source_kind)?,
            target_id,
            target_kind: required_text("relationship_target_kind", input.target_kind)?,
            target_hint: normalize_optional("target_hint", input.target_hint)?,
            resolution_state: required_text("resolution_state", input.resolution_state)?,
            confidence_basis_points: validate_confidence(input.confidence_basis_points)?,
            confidence_tier: required_text("confidence_tier", input.confidence_tier)?,
            evidence_path,
            evidence_line_range: input.evidence_line_range,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareRelationship`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareRelationshipInput {
    pub repository_id: String,
    pub source_scope: String,
    pub relationship_kind: String,
    pub source_id: String,
    pub source_kind: String,
    pub target_id: String,
    pub target_kind: String,
    pub target_hint: Option<String>,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub evidence_path: String,
    pub evidence_line_range: RepositoryCodeRange,
    pub created_graph_version: GraphVersion,
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
