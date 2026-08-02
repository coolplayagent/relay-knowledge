use serde::{Deserialize, Serialize};

use super::super::EvidenceSpan;
use super::{
    CodeGraphArtifact, ContextEntity, ContextGraphFact, RankingSignal, RerankSignal,
    RetrieverSource,
};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank: Option<RerankSignal>,
    pub score: f64,
}
