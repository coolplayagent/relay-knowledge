use serde::{Deserialize, Serialize};

use super::super::{EvidenceSpan, GraphVersion};
use super::{
    CodeGraphArtifact, ContextEntity, ContextGraphFact, ContextGraphPath, FreshnessPolicy,
    RankingSignal, RerankSignal, RetrievalBackendStatus, RetrieverSource, TraversalProvenanceTrace,
};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_trace: Option<TraversalProvenanceTrace>,
    pub items: Vec<ContextPackItem>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graph_paths: Vec<ContextGraphPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_artifact: Option<CodeGraphArtifact>,
    pub retriever_sources: Vec<RetrieverSource>,
    pub ranking: Vec<RankingSignal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank: Option<RerankSignal>,
}
