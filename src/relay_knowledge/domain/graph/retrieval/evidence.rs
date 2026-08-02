use serde::{Deserialize, Serialize};

use super::super::{ConfidenceScore, FactStatus, GraphVersionRange};

/// Entity projection retained with each context item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEntity {
    pub id: String,
    pub label: String,
}

/// Structured graph fact kind referenced from a context item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextGraphFactKind {
    Relation,
    Claim,
    Event,
}

impl ContextGraphFactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Relation => "relation",
            Self::Claim => "claim",
            Self::Event => "event",
        }
    }
}

/// Structured relation, claim, or event that supports a retrieval hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextGraphFact {
    pub fact_id: String,
    pub kind: ContextGraphFactKind,
    pub subject: String,
    pub predicate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: ConfidenceScore,
    pub status: FactStatus,
    pub version_range: GraphVersionRange,
}

/// Direct graph path evidence derived from a structured graph fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextGraphPath {
    pub path_id: String,
    pub nodes: Vec<String>,
    pub edges: Vec<ContextGraphPathEdge>,
}

/// One edge in a graph path returned through the context pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextGraphPathEdge {
    pub fact_id: String,
    pub kind: ContextGraphFactKind,
    pub from: String,
    pub predicate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: ConfidenceScore,
    pub status: FactStatus,
    pub version_range: GraphVersionRange,
}

impl ContextGraphPath {
    /// Builds a one-hop path from a persisted structured fact.
    pub fn from_fact(fact: &ContextGraphFact) -> Self {
        let mut nodes = vec![fact.subject.clone()];
        if let Some(object) = &fact.object
            && !nodes.contains(object)
        {
            nodes.push(object.clone());
        }

        Self {
            path_id: format!("path:{}", fact.fact_id),
            nodes,
            edges: vec![ContextGraphPathEdge {
                fact_id: fact.fact_id.clone(),
                kind: fact.kind,
                from: fact.subject.clone(),
                predicate: fact.predicate.clone(),
                to: fact.object.clone(),
                evidence_ids: fact.evidence_ids.clone(),
                confidence: fact.confidence,
                status: fact.status,
                version_range: fact.version_range,
            }],
        }
    }
}

/// Code artifact category returned through the general GraphRAG context pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeGraphArtifactKind {
    Symbol,
    Chunk,
}

impl CodeGraphArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Symbol => "symbol",
            Self::Chunk => "chunk",
        }
    }
}

/// Code graph artifact tied to a shared retrieval result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeGraphArtifact {
    pub kind: CodeGraphArtifactKind,
    pub artifact_id: String,
    pub path: String,
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
