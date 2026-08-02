use serde::{Deserialize, Serialize};

use super::{RerankMode, RetrieverSource};

/// RRF constant used by Phase 1 hybrid retrieval.
pub const RECIPROCAL_RANK_FUSION_K: f64 = 60.0;

/// Per-retriever ranking signal preserved after fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankingSignal {
    pub source: RetrieverSource,
    pub rank: usize,
    pub score: f64,
    pub explanation: String,
}

/// Final rerank signal applied after hybrid retrieval fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankSignal {
    pub mode: RerankMode,
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

/// Diagnostics for post-fusion reranking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankDiagnostics {
    pub requested_mode: RerankMode,
    pub effective_mode: RerankMode,
    pub algorithm: String,
    pub candidate_count: usize,
    pub returned_count: usize,
    pub degraded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
