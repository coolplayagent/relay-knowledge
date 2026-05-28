use serde::{Deserialize, Serialize};

use super::{
    DomainError, FreshnessPolicy, GraphVersion, RepositoryCodeRange, error::required_text,
};

/// Query kind for repository-scoped software global model facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareGlobalKind {
    Dependencies,
    Sdks,
    All,
}

impl SoftwareGlobalKind {
    /// Stable CLI, API, and storage-facing representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::Sdks => "sdks",
            Self::All => "all",
        }
    }
}

/// Repository-scoped software global model query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalRequest {
    pub repository: super::CodeRepositorySelector,
    pub kind: SoftwareGlobalKind,
    pub freshness_policy: FreshnessPolicy,
    pub limit: usize,
}

impl SoftwareGlobalRequest {
    /// Validates the requested result bound while preserving repository scope.
    pub fn new(
        repository: super::CodeRepositorySelector,
        kind: SoftwareGlobalKind,
        freshness_policy: FreshnessPolicy,
        limit: usize,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=500 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 500 or less")),
        };

        Ok(Self {
            repository,
            kind,
            freshness_policy,
            limit,
        })
    }
}

/// Projected dependency component from repository manifests and lockfiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareComponent {
    pub component_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub ecosystem: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_version: Option<String>,
    pub dependency_group: String,
    pub source_kind: String,
    pub relationship_state: String,
    pub language_id: String,
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
            language_id: required_text("language_id", input.language_id)?,
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
    pub name: String,
    pub requirement: Option<String>,
    pub resolved_version: Option<String>,
    pub dependency_group: String,
    pub source_kind: String,
    pub relationship_state: String,
    pub language_id: String,
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

/// Freshness and count summary for the software global projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalStatus {
    pub repository_id: String,
    pub source_scope: String,
    pub projected_graph_version: GraphVersion,
    pub stale: bool,
    pub component_count: usize,
    pub sdk_usage_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Projected software global facts for one repository scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalProjection {
    pub status: SoftwareGlobalStatus,
    pub components: Vec<SoftwareComponent>,
    pub sdk_usages: Vec<SoftwareSdkUsage>,
}

fn normalize_optional(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, DomainError> {
    value.map(|text| required_text(field, text)).transpose()
}

fn validate_confidence(value: u16) -> Result<u16, DomainError> {
    if value > 10_000 {
        return Err(DomainError::invalid(
            "confidence",
            "must be between 0 and 10000 basis points",
        ));
    }

    Ok(value)
}

fn stable_software_id<'a>(prefix: &str, parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("{prefix}:{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_identity_includes_scope_and_version() {
        let base = component_input("scope-a", Some("1.0.0"));
        let component = SoftwareComponent::new(base).expect("component should validate");
        let changed = SoftwareComponent::new(component_input("scope-b", Some("1.0.0")))
            .expect("component should validate");

        assert_ne!(component.component_id, changed.component_id);
    }

    #[test]
    fn component_identity_preserves_duplicate_evidence_rows() {
        let first = SoftwareComponent::new(component_input("scope-a", Some("1.0.0")))
            .expect("component should validate");
        let mut second_input = component_input("scope-a", Some("1.0.0"));
        second_input.evidence_path = "crates/core/Cargo.toml".to_owned();
        second_input.evidence_line_range = RepositoryCodeRange { start: 9, end: 9 };
        let second = SoftwareComponent::new(second_input).expect("component should validate");

        assert_ne!(first.component_id, second.component_id);
    }

    #[test]
    fn component_rejects_empty_name_and_invalid_confidence() {
        let mut input = component_input("scope-a", None);
        input.name = " ".to_owned();
        assert_eq!(
            SoftwareComponent::new(input)
                .expect_err("empty name should fail")
                .field,
            "component_name"
        );

        let mut input = component_input("scope-a", None);
        input.confidence_basis_points = 10_001;
        assert_eq!(
            SoftwareComponent::new(input)
                .expect_err("bad confidence should fail")
                .field,
            "confidence"
        );
    }

    #[test]
    fn sdk_usage_preserves_unresolved_target_hint() {
        let usage = SoftwareSdkUsage::new(SoftwareSdkUsageInput {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            language_id: "cpp".to_owned(),
            module: "#include <securec.h>".to_owned(),
            target_hint: Some("securec.h".to_owned()),
            resolution_state: "unresolved".to_owned(),
            evidence_path: "src/main.cc".to_owned(),
            evidence_line_range: RepositoryCodeRange { start: 3, end: 3 },
            confidence_basis_points: 2500,
            created_graph_version: GraphVersion::new(7),
        })
        .expect("usage should validate");

        assert_eq!(usage.target_hint.as_deref(), Some("securec.h"));
    }

    #[test]
    fn sdk_usage_identity_preserves_repeated_evidence_rows() {
        let first = SoftwareSdkUsage::new(sdk_usage_input(3)).expect("usage should validate");
        let second = SoftwareSdkUsage::new(sdk_usage_input(9)).expect("usage should validate");

        assert_ne!(first.usage_id, second.usage_id);
    }

    fn component_input(scope: &str, version: Option<&str>) -> SoftwareComponentInput {
        SoftwareComponentInput {
            repository_id: "repo".to_owned(),
            source_scope: scope.to_owned(),
            ecosystem: "cargo".to_owned(),
            name: "serde".to_owned(),
            requirement: Some("1".to_owned()),
            resolved_version: version.map(str::to_owned),
            dependency_group: "normal".to_owned(),
            source_kind: "manifest".to_owned(),
            relationship_state: "declared".to_owned(),
            language_id: "rust".to_owned(),
            evidence_path: "Cargo.toml".to_owned(),
            evidence_line_range: RepositoryCodeRange { start: 1, end: 1 },
            confidence_basis_points: 10_000,
            created_graph_version: GraphVersion::new(1),
        }
    }

    fn sdk_usage_input(line: u32) -> SoftwareSdkUsageInput {
        SoftwareSdkUsageInput {
            repository_id: "repo".to_owned(),
            source_scope: "scope".to_owned(),
            language_id: "cpp".to_owned(),
            module: "#include <securec.h>".to_owned(),
            target_hint: Some("securec.h".to_owned()),
            resolution_state: "unresolved".to_owned(),
            evidence_path: "src/main.cc".to_owned(),
            evidence_line_range: RepositoryCodeRange {
                start: line,
                end: line,
            },
            confidence_basis_points: 2500,
            created_graph_version: GraphVersion::new(7),
        }
    }
}
