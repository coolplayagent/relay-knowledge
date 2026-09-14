use serde::{Deserialize, Serialize};

use crate::{
    api::ApiMetadata,
    domain::{
        FreshnessPolicy, FusionDiagnostics, IndexCursor, IndexRefreshDiagnostics, IndexStatus,
        RerankDiagnostics, RetrievalBackendStatus, RetrievalBudgetUsed, RetrievalHit,
        RetrievalMode, RetrievedContextPack,
    },
};

/// Hybrid retrieval request over graph facts and derived indexes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HybridRetrievalRequest {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub limit: usize,
    pub freshness: FreshnessPolicy,
}

impl HybridRetrievalRequest {
    /// Creates a bounded default retrieval request for human-facing interfaces.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            source_scope: None,
            limit: 10,
            freshness: FreshnessPolicy::default(),
        }
    }
}

/// Retrieval response with freshness and degradation information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HybridRetrievalResponse {
    pub metadata: ApiMetadata,
    pub context_pack: RetrievedContextPack,
    pub retrieval_mode: RetrievalMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub freshness: FreshnessPolicy,
    pub results: Vec<RetrievalHit>,
    pub fusion: FusionDiagnostics,
    pub rerank: RerankDiagnostics,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backend_statuses: Vec<RetrievalBackendStatus>,
    pub truncated: bool,
    pub budget_used: RetrievalBudgetUsed,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub indexes: Vec<IndexStatus>,
    #[serde(default)]
    pub index_cursors: Vec<IndexCursor>,
    #[serde(default)]
    pub index_refresh: IndexRefreshDiagnostics,
}

#[cfg(test)]
#[path = "retrieval_tests.rs"]
mod tests;
