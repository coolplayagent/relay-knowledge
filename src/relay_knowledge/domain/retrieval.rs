use serde::{Deserialize, Serialize};

use super::{ConfidenceScore, EvidenceSpan, FactStatus, GraphVersion, GraphVersionRange};

/// RRF constant used by Phase 1 hybrid retrieval.
pub const RECIPROCAL_RANK_FUSION_K: f64 = 60.0;

/// Freshness policy for hybrid retrieval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessPolicy {
    #[default]
    AllowStale,
    WaitUntilFresh,
    GraphOnly,
}

/// Retrieval path used to satisfy a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    Hybrid,
    GraphOnly,
}

/// Retrieval source that contributed to a fused context result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrieverSource {
    Bm25,
    GraphEvidence,
    CodeGraph,
    Semantic,
    Vector,
    GraphPath,
    Temporal,
    CommunitySummary,
}

/// Availability state for optional retrieval backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalBackendState {
    Available,
    Degraded,
    Unavailable,
}

/// Per-backend status preserved so callers can distinguish fallback from
/// complete hybrid retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalBackendStatus {
    pub source: RetrieverSource,
    pub state: RetrievalBackendState,
    pub scope_post_filter: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_graph_version: Option<GraphVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl RetrieverSource {
    /// Stable API representation used in ranking diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bm25 => "bm25",
            Self::GraphEvidence => "graph_evidence",
            Self::CodeGraph => "code_graph",
            Self::Semantic => "semantic",
            Self::Vector => "vector",
            Self::GraphPath => "graph_path",
            Self::Temporal => "temporal",
            Self::CommunitySummary => "community_summary",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retriever_source_labels_match_wire_values() {
        assert_eq!(RetrieverSource::Bm25.as_str(), "bm25");
        assert_eq!(RetrieverSource::GraphEvidence.as_str(), "graph_evidence");
        assert_eq!(RetrieverSource::CodeGraph.as_str(), "code_graph");
        assert_eq!(RetrieverSource::Semantic.as_str(), "semantic");
        assert_eq!(RetrieverSource::Vector.as_str(), "vector");
        assert_eq!(RetrieverSource::GraphPath.as_str(), "graph_path");
        assert_eq!(RetrieverSource::Temporal.as_str(), "temporal");
        assert_eq!(
            RetrieverSource::CommunitySummary.as_str(),
            "community_summary"
        );
    }
}

/// Per-retriever ranking signal preserved after fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankingSignal {
    pub source: RetrieverSource,
    pub rank: usize,
    pub score: f64,
    pub explanation: String,
}

/// Budget actually consumed by retrieval context packing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalBudgetUsed {
    pub limit: usize,
    pub candidate_count: usize,
    pub returned_count: usize,
    pub context_bytes: usize,
}

/// Diagnostics for reciprocal-rank fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FusionDiagnostics {
    pub algorithm: String,
    pub k: f64,
    pub candidate_count: usize,
}

/// A compact, auditable context pack for agent and UI adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedContextPack {
    pub graph_version: GraphVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub freshness: FreshnessPolicy,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backend_statuses: Vec<RetrievalBackendStatus>,
    pub items: Vec<ContextPackItem>,
}

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

/// Code artifact category returned through the general GraphRAG context pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeGraphArtifactKind {
    Symbol,
    Chunk,
}

/// Code graph artifact tied to a shared retrieval result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeGraphArtifact {
    pub kind: CodeGraphArtifactKind,
    pub artifact_id: String,
    pub path: String,
}

/// Context-pack item tied to a retrieval hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPackItem {
    pub result_id: String,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_span: Option<EvidenceSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<ContextEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graph_facts: Vec<ContextGraphFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_artifact: Option<CodeGraphArtifact>,
    pub retriever_sources: Vec<RetrieverSource>,
    pub ranking: Vec<RankingSignal>,
}

/// A context item returned by retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalHit {
    pub evidence_id: String,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_span: Option<EvidenceSpan>,
    pub content: String,
    pub entity_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<ContextEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graph_facts: Vec<ContextGraphFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_artifact: Option<CodeGraphArtifact>,
    pub retriever_sources: Vec<RetrieverSource>,
    pub ranking: Vec<RankingSignal>,
    pub score: f64,
}
