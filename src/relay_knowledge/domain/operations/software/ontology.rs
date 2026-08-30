use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::super::{DomainError, GraphVersion, RepositoryCodeRange, error::required_text};
use super::validation::{normalize_optional, stable_software_id};

/// Version of the repository software ontology contract exposed on every read model.
pub const SOFTWARE_ONTOLOGY_VERSION: &str = "1.0.0";

const MAX_ENTITY_ATTRIBUTES: usize = 64;
const MAX_EVIDENCE_REFS: usize = 64;

/// Stable and occurrence entity kinds in the software ontology contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareEntityKind {
    Domain,
    SoftwareSystem,
    Component,
    Api,
    Resource,
    Configuration,
    BuildDefinition,
    DeploymentUnit,
    RuntimeService,
    TestCase,
    ReleaseArtifact,
    PackageComponent,
    Sdk,
    DocumentationUnit,
    Pipeline,
    BuildJob,
    RepositorySnapshot,
    FileRevision,
    BuildRun,
    DeploymentRevision,
    RuntimeObservation,
}

impl SoftwareEntityKind {
    /// Stable storage and wire value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::SoftwareSystem => "software_system",
            Self::Component => "component",
            Self::Api => "api",
            Self::Resource => "resource",
            Self::Configuration => "configuration",
            Self::BuildDefinition => "build_definition",
            Self::DeploymentUnit => "deployment_unit",
            Self::RuntimeService => "runtime_service",
            Self::TestCase => "test_case",
            Self::ReleaseArtifact => "release_artifact",
            Self::PackageComponent => "package_component",
            Self::Sdk => "sdk",
            Self::DocumentationUnit => "documentation_unit",
            Self::Pipeline => "pipeline",
            Self::BuildJob => "build_job",
            Self::RepositorySnapshot => "repository_snapshot",
            Self::FileRevision => "file_revision",
            Self::BuildRun => "build_run",
            Self::DeploymentRevision => "deployment_revision",
            Self::RuntimeObservation => "runtime_observation",
        }
    }

    /// Parses persisted contract values without accepting unknown future kinds.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "domain" => Some(Self::Domain),
            "software_system" => Some(Self::SoftwareSystem),
            "component" => Some(Self::Component),
            "api" => Some(Self::Api),
            "resource" => Some(Self::Resource),
            "configuration" => Some(Self::Configuration),
            "build_definition" => Some(Self::BuildDefinition),
            "deployment_unit" => Some(Self::DeploymentUnit),
            "runtime_service" => Some(Self::RuntimeService),
            "test_case" => Some(Self::TestCase),
            "release_artifact" => Some(Self::ReleaseArtifact),
            "package_component" => Some(Self::PackageComponent),
            "sdk" => Some(Self::Sdk),
            "documentation_unit" => Some(Self::DocumentationUnit),
            "pipeline" => Some(Self::Pipeline),
            "build_job" => Some(Self::BuildJob),
            "repository_snapshot" => Some(Self::RepositorySnapshot),
            "file_revision" => Some(Self::FileRevision),
            "build_run" => Some(Self::BuildRun),
            "deployment_revision" => Some(Self::DeploymentRevision),
            "runtime_observation" => Some(Self::RuntimeObservation),
            _ => None,
        }
    }

    /// Snapshot and event instances intentionally carry source-scope identity.
    pub const fn is_occurrence_kind(self) -> bool {
        matches!(
            self,
            Self::RepositorySnapshot
                | Self::FileRevision
                | Self::BuildRun
                | Self::DeploymentRevision
                | Self::RuntimeObservation
        )
    }
}

/// Controlled provenance source categories used by authority policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareSourceKind {
    Manifest,
    Lockfile,
    Sbom,
    BuildAttestation,
    BuildFile,
    Ci,
    Iac,
    ServiceDefinition,
    ApiSchema,
    Documentation,
    Code,
    Test,
    Runtime,
    Connector,
}

impl SoftwareSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manifest => "manifest",
            Self::Lockfile => "lockfile",
            Self::Sbom => "sbom",
            Self::BuildAttestation => "build_attestation",
            Self::BuildFile => "build_file",
            Self::Ci => "ci",
            Self::Iac => "iac",
            Self::ServiceDefinition => "service_definition",
            Self::ApiSchema => "api_schema",
            Self::Documentation => "documentation",
            Self::Code => "code",
            Self::Test => "test",
            Self::Runtime => "runtime",
            Self::Connector => "connector",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manifest" => Some(Self::Manifest),
            "lockfile" => Some(Self::Lockfile),
            "sbom" => Some(Self::Sbom),
            "build_attestation" => Some(Self::BuildAttestation),
            "build_file" => Some(Self::BuildFile),
            "ci" => Some(Self::Ci),
            "iac" => Some(Self::Iac),
            "service_definition" => Some(Self::ServiceDefinition),
            "api_schema" => Some(Self::ApiSchema),
            "documentation" => Some(Self::Documentation),
            "code" => Some(Self::Code),
            "test" => Some(Self::Test),
            "runtime" => Some(Self::Runtime),
            "connector" => Some(Self::Connector),
            _ => None,
        }
    }
}

/// One immutable source location supporting an entity occurrence or statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareEvidenceRef {
    pub evidence_id: String,
    pub source_scope: String,
    pub path: String,
    pub line_range: RepositoryCodeRange,
}

impl SoftwareEvidenceRef {
    /// Creates a deterministic evidence identity without reading live source bytes.
    pub fn new(
        source_scope: impl Into<String>,
        path: impl Into<String>,
        line_range: RepositoryCodeRange,
    ) -> Result<Self, DomainError> {
        let source_scope = required_text("source_scope", source_scope.into())?;
        let path = required_text("evidence_path", path.into())?;
        if line_range.start == 0 || line_range.end < line_range.start {
            return Err(DomainError::invalid(
                "evidence_line_range",
                "must use positive ordered line numbers",
            ));
        }
        let start = line_range.start.to_string();
        let end = line_range.end.to_string();
        Ok(Self {
            evidence_id: stable_software_id(
                "software_evidence",
                [
                    source_scope.as_str(),
                    path.as_str(),
                    start.as_str(),
                    end.as_str(),
                ],
            ),
            source_scope,
            path,
            line_range,
        })
    }
}

/// One observed occurrence of a stable software entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareEntity {
    pub entity_key: String,
    pub occurrence_id: String,
    pub repository_id: String,
    pub source_scope: String,
    pub entity_kind: SoftwareEntityKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    pub source_kind: SoftwareSourceKind,
    pub evidence_refs: Vec<SoftwareEvidenceRef>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    pub created_graph_version: GraphVersion,
}

impl SoftwareEntity {
    /// Separates a commit-independent entity key from its snapshot occurrence id.
    pub fn new(input: SoftwareEntityInput) -> Result<Self, DomainError> {
        let repository_id = required_text("repository_id", input.repository_id)?;
        let source_scope = required_text("source_scope", input.source_scope)?;
        let name = required_text("software_entity_name", input.name)?;
        let namespace = normalize_optional("software_entity_namespace", input.namespace)?;
        validate_evidence_refs(&input.evidence_refs)?;
        validate_attributes(&input.attributes)?;

        let kind = input.entity_kind.as_str();
        let namespace_part = namespace.as_deref().unwrap_or("");
        let entity_key = if input.entity_kind.is_occurrence_kind() {
            stable_software_id(
                "software_entity",
                [
                    repository_id.as_str(),
                    kind,
                    namespace_part,
                    name.as_str(),
                    source_scope.as_str(),
                ],
            )
        } else {
            stable_software_id(
                "software_entity",
                [repository_id.as_str(), kind, namespace_part, name.as_str()],
            )
        };
        let mut occurrence_parts = vec![entity_key.as_str(), source_scope.as_str()];
        occurrence_parts.extend(
            input
                .evidence_refs
                .iter()
                .map(|evidence| evidence.evidence_id.as_str()),
        );
        let occurrence_id = stable_software_id("software_occurrence", occurrence_parts);

        Ok(Self {
            entity_key,
            occurrence_id,
            repository_id,
            source_scope,
            entity_kind: input.entity_kind,
            name,
            namespace,
            source_kind: input.source_kind,
            evidence_refs: input.evidence_refs,
            attributes: input.attributes,
            created_graph_version: input.created_graph_version,
        })
    }
}

/// Constructor input for `SoftwareEntity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftwareEntityInput {
    pub repository_id: String,
    pub source_scope: String,
    pub entity_kind: SoftwareEntityKind,
    pub name: String,
    pub namespace: Option<String>,
    pub source_kind: SoftwareSourceKind,
    pub evidence_refs: Vec<SoftwareEvidenceRef>,
    pub attributes: BTreeMap<String, String>,
    pub created_graph_version: GraphVersion,
}

fn validate_evidence_refs(evidence_refs: &[SoftwareEvidenceRef]) -> Result<(), DomainError> {
    if evidence_refs.len() > MAX_EVIDENCE_REFS {
        return Err(DomainError::invalid(
            "evidence_refs",
            format!("must contain {MAX_EVIDENCE_REFS} entries or fewer"),
        ));
    }
    Ok(())
}

fn validate_attributes(attributes: &BTreeMap<String, String>) -> Result<(), DomainError> {
    if attributes.len() > MAX_ENTITY_ATTRIBUTES {
        return Err(DomainError::invalid(
            "attributes",
            format!("must contain {MAX_ENTITY_ATTRIBUTES} entries or fewer"),
        ));
    }
    for (key, value) in attributes {
        required_text("software_entity_attribute_key", key.clone())?;
        required_text("software_entity_attribute_value", value.clone())?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "ontology_tests.rs"]
mod tests;
