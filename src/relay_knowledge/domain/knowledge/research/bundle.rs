//! Lossless authored graph envelopes and stable concept identity.

use super::{ResearchArtifact, catalog::bounded_text, validate_sha256};
use crate::domain::{DomainError, EvidenceSpan, SourceScope};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_BUNDLE_NODES: usize = 512;
pub const MAX_BUNDLE_EDGES: usize = 2048;
pub const MAX_BUNDLE_EVIDENCE: usize = 512;

/// The original graph stays intact inside a versioned, explicitly scoped envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredEvidenceBundle {
    pub schema_version: u16,
    pub id: String,
    pub source_scope: String,
    pub graph: AuthoredEvidenceGraph,
    pub evidence: Vec<AuthoredEvidencePin>,
    pub supersedes: Option<String>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredEvidenceGraph {
    pub nodes: Vec<AuthoredEvidenceNode>,
    pub edges: Vec<AuthoredEvidenceEdge>,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredEvidenceNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredEvidenceEdge {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub source: String,
    pub target: String,
    pub relation: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(flatten)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredEvidencePin {
    pub id: String,
    pub source_scope: String,
    pub artifact: ResearchArtifact,
    pub span: Option<EvidenceSpan>,
    pub interpretation: EvidenceInterpretation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceInterpretation {
    SourceStatement,
    AuthorAnalysis,
    Hypothesis,
    UserScopeConfirmation,
    HistoricalDisambiguation,
}

impl AuthoredEvidenceBundle {
    pub fn validate_shape(&self) -> Result<(), DomainError> {
        if self.schema_version != 1 {
            return Err(DomainError::invalid("schema_version", "must be 1"));
        }
        bounded_text(&self.id, "bundle.id", 128)?;
        validate_scope(&self.source_scope)?;
        if self.graph.nodes.is_empty()
            || self.graph.nodes.len() > MAX_BUNDLE_NODES
            || self.graph.edges.len() > MAX_BUNDLE_EDGES
            || self.evidence.len() > MAX_BUNDLE_EVIDENCE
            || self.aliases.len() > MAX_BUNDLE_NODES
        {
            return Err(DomainError::invalid(
                "bundle",
                "requires 1..512 nodes, at most 2048 edges, 512 evidence pins and 512 aliases",
            ));
        }
        if let Some(previous) = &self.supersedes {
            validate_sha256(previous)?;
        }
        for node in &self.graph.nodes {
            bounded_text(&node.id, "node.id", 256)?;
            bounded_text(&node.kind, "node.kind", 256)?;
            bounded_text(&node.label, "node.label", 1024)?;
        }
        for edge in &self.graph.edges {
            if let Some(id) = &edge.id {
                bounded_text(id, "edge.id", 256)?;
            }
            bounded_text(&edge.source, "edge.source", 256)?;
            bounded_text(&edge.target, "edge.target", 256)?;
            bounded_text(&edge.relation, "edge.relation", 256)?;
            if edge.evidence.len() > 32 {
                return Err(DomainError::invalid(
                    "edge.evidence",
                    "at most 32 references per relation",
                ));
            }
        }
        for pin in &self.evidence {
            bounded_text(&pin.id, "evidence.id", 4096)?;
            validate_scope(&pin.source_scope)?;
            pin.artifact.validate()?;
            if let Some(span) = pin.span {
                EvidenceSpan::new(
                    span.start_byte,
                    span.end_byte,
                    span.start_line,
                    span.end_line,
                )?;
            }
        }
        let ids: BTreeSet<_> = self.graph.nodes.iter().map(|node| &node.id).collect();
        for (alias, target) in &self.aliases {
            bounded_text(alias, "alias", 1024)?;
            if !ids.contains(target) {
                return Err(DomainError::invalid(
                    "alias",
                    "target must be a stable node id",
                ));
            }
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|error| DomainError::invalid("bundle", error.to_string()))?;
        if bytes.len() > super::MAX_RESEARCH_JSON_BYTES {
            return Err(DomainError::invalid("bundle", "exceeds 2 MiB"));
        }
        Ok(())
    }

    /// Returns a proposed revision without touching old facts or rewriting evidence.
    pub fn revise_label(
        &self,
        node_id: &str,
        label: &str,
        prior_digest: String,
    ) -> Result<(Self, Vec<usize>), DomainError> {
        self.validate_shape()?;
        bounded_text(label, "label", 1024)?;
        validate_sha256(&prior_digest)?;
        let mut revision = self.clone();
        let node = revision
            .graph
            .nodes
            .iter_mut()
            .find(|node| node.id == node_id)
            .ok_or_else(|| DomainError::invalid("node", "unknown stable node id"))?;
        if revision
            .aliases
            .get(&node.label)
            .is_some_and(|id| id != node_id)
        {
            return Err(DomainError::invalid(
                "alias",
                "previous label already maps to another concept",
            ));
        }
        revision.aliases.insert(node.label.clone(), node.id.clone());
        node.label = label.to_owned();
        revision.supersedes = Some(prior_digest);
        revision.validate_shape()?;
        let affected = self
            .graph
            .edges
            .iter()
            .enumerate()
            .filter_map(|(index, edge)| {
                (edge.source == node_id || edge.target == node_id).then_some(index)
            })
            .collect();
        Ok((revision, affected))
    }
}

fn validate_scope(value: &str) -> Result<(), DomainError> {
    bounded_text(value, "source_scope", 4096)?;
    if SourceScope::parse(value)?.as_str() != value {
        return Err(DomainError::invalid(
            "source_scope",
            "must already be normalized without surrounding whitespace",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "bundle_tests.rs"]
mod tests;
