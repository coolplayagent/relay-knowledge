use serde::{Deserialize, Serialize};

use crate::{
    api::ApiMetadata,
    domain::{
        CommitReceipt, ConfidenceScore, EvidenceExtractionMetadata, EvidenceModality, EvidenceSpan,
        ExtractionDiagnostic, FactStatus, GraphVersionRange, IndexStatus, LayoutRegion,
    },
};

/// Evidence item supplied to the ingest API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestEvidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<EvidenceSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceScore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<FactStatus>,
    pub content: String,
    #[serde(default)]
    pub entity_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction: Option<IngestEvidenceExtraction>,
}

/// Optional multimodal extraction metadata supplied with an evidence item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestEvidenceExtraction {
    pub modality: EvidenceModality,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extractor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extractor_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_evidence_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_region: Option<LayoutRegion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_dimension: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<ExtractionDiagnostic>,
}

impl IngestEvidenceExtraction {
    /// Converts API metadata into the domain extraction contract.
    pub fn into_domain_metadata(self) -> EvidenceExtractionMetadata {
        EvidenceExtractionMetadata {
            modality: self.modality,
            source_uri: self.source_uri,
            source_hash: self.source_hash,
            media_hash: self.media_hash,
            extractor: self.extractor,
            extractor_version: self.extractor_version,
            observed_at: self.observed_at,
            parent_evidence_id: self.parent_evidence_id,
            layout_region: self.layout_region,
            embedding_model: self.embedding_model,
            embedding_dimension: self.embedding_dimension,
            diagnostic: self
                .diagnostic
                .unwrap_or_else(|| EvidenceExtractionMetadata::text_span().diagnostic),
        }
    }
}

/// Structured relation supplied to the ingest API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestRelation {
    pub id: String,
    pub source_entity_label: String,
    pub relation_type: String,
    pub target_entity_label: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceScore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<FactStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_range: Option<GraphVersionRange>,
}

/// Structured claim supplied to the ingest API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestClaim {
    pub id: String,
    pub subject_entity_label: String,
    pub predicate: String,
    pub object: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceScore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<FactStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_range: Option<GraphVersionRange>,
}

/// Structured event supplied to the ingest API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestEvent {
    pub id: String,
    pub event_type: String,
    #[serde(default)]
    pub entity_labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<ConfidenceScore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<FactStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_range: Option<GraphVersionRange>,
}

/// Ingest request shared by CLI, Web, HTTP, and future agent adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestRequest {
    pub source_scope: String,
    #[serde(default)]
    pub evidence: Vec<IngestEvidence>,
    #[serde(default)]
    pub relations: Vec<IngestRelation>,
    #[serde(default)]
    pub claims: Vec<IngestClaim>,
    #[serde(default)]
    pub events: Vec<IngestEvent>,
}

/// Ingest response with committed graph and refreshed index versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestResponse {
    pub metadata: ApiMetadata,
    pub receipt: CommitReceipt,
    pub indexes: Vec<IndexStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_refresh_error: Option<String>,
}

/// Maintenance-worker output for derived OCR, caption, table, layout, or image embeddings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultimodalExtractionRequest {
    pub source_scope: String,
    pub parent_evidence_id: String,
    pub derived_evidence: Vec<IngestEvidence>,
}

/// Commit result for a bounded multimodal maintenance batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultimodalExtractionResponse {
    pub metadata: ApiMetadata,
    pub parent_evidence_id: String,
    pub derived_evidence_count: usize,
    pub receipt: CommitReceipt,
    pub indexes: Vec<IndexStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_refresh_error: Option<String>,
}

#[cfg(test)]
#[path = "ingestion_tests.rs"]
mod tests;
