use serde::{Deserialize, Serialize};

use crate::domain::{FactStatus, GraphVersion};

use super::{
    BusinessAlias, BusinessSemantics, BusinessTechnicalMappingDefinition, BusinessTermStatus,
    OntologyIdentity, TechnicalTargetKind,
};

/// One route-authorized authored glossary loaded from an immutable repository snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeSource {
    pub source_id: String,
    pub source_path: String,
    pub authority_rank: usize,
    pub content_digest: String,
    pub glossary: super::BusinessGlossary,
}

/// Prepared projection input written by the repository index writer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeProjectionInput {
    pub repository_id: String,
    pub source_scope: String,
    pub resolved_commit_sha: String,
    pub sources: Vec<BusinessKnowledgeSource>,
}

/// Provenance retained for every accepted authored business fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessEvidence {
    pub evidence_id: String,
    pub source_id: String,
    pub source_path: String,
    pub source_digest: String,
    pub resolved_commit_sha: String,
    pub line_start: u32,
    pub line_end: u32,
    pub confidence_basis_points: u16,
    pub lifecycle: FactStatus,
    pub valid_from_graph_version: GraphVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until_graph_version: Option<GraphVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessDefinitionFact {
    pub definition: String,
    pub preferred: bool,
    pub evidence: BusinessEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeConflict {
    pub predicate: String,
    pub competing_values: Vec<String>,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessDomain {
    pub identity: OntologyIdentity,
    pub entity_id: String,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub evidence: BusinessEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessTechnicalMapping {
    #[serde(flatten)]
    pub definition: BusinessTechnicalMappingDefinition,
    pub resolution_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_id: Option<String>,
    pub target_hint: String,
    pub evidence: BusinessEvidence,
}

impl BusinessTechnicalMapping {
    pub fn is_resolved(&self) -> bool {
        self.resolution_state == "resolved" && self.resolved_id.is_some()
    }

    pub fn query_seed(&self) -> &str {
        self.resolved_id
            .as_deref()
            .unwrap_or(self.target_hint.as_str())
    }

    pub fn target_kind(&self) -> TechnicalTargetKind {
        self.definition.target_kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessTerm {
    pub identity: OntologyIdentity,
    pub entity_id: String,
    pub id: String,
    pub domain_id: String,
    pub canonical_name: String,
    pub language: String,
    pub status: BusinessTermStatus,
    pub definitions: Vec<BusinessDefinitionFact>,
    pub aliases: Vec<BusinessAlias>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub semantics: Vec<BusinessSemantics>,
    pub conflicts: Vec<BusinessKnowledgeConflict>,
    pub mappings: Vec<BusinessTechnicalMapping>,
}

/// Projection freshness and bounded cardinality summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeStatus {
    pub repository_id: String,
    pub source_scope: String,
    pub resolved_commit_sha: String,
    pub projected_graph_version: GraphVersion,
    pub stale: bool,
    pub source_count: usize,
    pub domain_count: usize,
    pub term_count: usize,
    pub mapping_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Storage projection before API metadata and repository scope are attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusinessKnowledgeProjection {
    pub status: BusinessKnowledgeStatus,
    pub resolution: super::BusinessKnowledgeResolution,
    pub domains: Vec<BusinessDomain>,
    pub terms: Vec<BusinessTerm>,
}
