use serde::{Deserialize, Serialize};

use super::GraphVersion;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrieverSource {
    Bm25,
    GraphEvidence,
    CodeGraph,
    Semantic,
    Vector,
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
    pub items: Vec<ContextPackItem>,
}

/// Context-pack item tied to a retrieval hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPackItem {
    pub result_id: String,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
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
    pub content: String,
    pub entity_labels: Vec<String>,
    pub retriever_sources: Vec<RetrieverSource>,
    pub ranking: Vec<RankingSignal>,
    pub score: f64,
}
