use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        CodeImpactRequest, CodeIndexSummary, CodeRepositoryRegistration, CodeRepositorySelector,
        CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest, CommitReceipt,
        ConfidenceScore, EvidenceExtractionMetadata, EvidenceModality, EvidenceSpan,
        ExtractionDiagnostic, FactStatus, FreshnessPolicy, FusionDiagnostics, GraphVersionRange,
        IndexKind, IndexStatus, LayoutRegion, RetrievalBackendStatus, RetrievalBudgetUsed,
        RetrievalHit, RetrievalMode, RetrievedContextPack,
    },
    storage::{GraphInspection, IndexCursor, IndexRefreshDiagnostics},
};

use super::{AgentProtocolStatus, ApiMetadata, RuntimeStatus};

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backend_statuses: Vec<RetrievalBackendStatus>,
    pub truncated: bool,
    pub budget_used: RetrievalBudgetUsed,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub indexes: Vec<IndexStatus>,
}

/// Graph inspection request with optional scope filtering reserved for adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphInspectionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
}

/// Graph inspection response for diagnostics and agent adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphInspectionResponse {
    pub metadata: ApiMetadata,
    pub graph: GraphInspection,
}

/// Index refresh request. Empty `kinds` means all v1 index families.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRefreshRequest {
    #[serde(default)]
    pub kinds: Vec<IndexKind>,
}

/// Index refresh response after metadata is updated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRefreshResponse {
    pub metadata: ApiMetadata,
    pub indexes: Vec<IndexStatus>,
    pub index_cursors: Vec<IndexCursor>,
    pub diagnostics: IndexRefreshDiagnostics,
}

/// Service manager status surfaced without exposing platform-specific handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatusResponse {
    pub metadata: ApiMetadata,
    pub service_name: String,
    pub mode: String,
    pub background_enabled: bool,
    pub silent_updates_enabled: bool,
    pub service_definition_path: String,
    pub index_refresh: IndexRefreshDiagnostics,
    pub agent_protocols: AgentProtocolStatus,
}

/// Startup recovery report for resident service mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceRecoveryReport {
    pub metadata: ApiMetadata,
    pub graph_version: u64,
    pub stale_index_kinds: Vec<IndexKind>,
    pub refreshed_index_kinds: Vec<IndexKind>,
    pub index_lag_max: u64,
    pub task_queue_depth: usize,
    pub dead_letter_count: usize,
    pub heartbeat_state: String,
}

/// Aggregated health response for CLI/Web/service diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub metadata: ApiMetadata,
    pub healthy: bool,
    pub graph: GraphInspection,
    pub indexes: Vec<IndexStatus>,
    pub index_cursors: Vec<IndexCursor>,
    pub index_refresh: IndexRefreshDiagnostics,
    pub runtime: RuntimeStatus,
}

/// Code repository registration request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRegisterRequest {
    pub root_path: String,
    pub alias: String,
    #[serde(default)]
    pub path_filters: Vec<String>,
    #[serde(default)]
    pub language_filters: Vec<String>,
}

/// Code repository registration response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRegisterResponse {
    pub metadata: ApiMetadata,
    pub registration: CodeRepositoryRegistration,
    pub status: CodeRepositoryStatus,
}

/// Code repository index response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryIndexResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub summary: CodeIndexSummary,
    pub status: CodeRepositoryStatus,
}

/// Code repository scope and index metadata attached to code responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryScopeMetadata {
    pub repository_id: String,
    pub alias: String,
    pub requested_ref: String,
    pub resolved_commit_sha: String,
    pub tree_hash: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub index_versions: Vec<String>,
    pub stale: bool,
}

impl CodeRepositoryScopeMetadata {
    /// Builds stable scope metadata from the selected repository snapshot.
    pub fn from_status(
        status: &CodeRepositoryStatus,
        selector: &CodeRepositorySelector,
        requested_ref: impl Into<String>,
    ) -> Self {
        Self {
            repository_id: status.repository_id.clone(),
            alias: status.alias.clone(),
            requested_ref: requested_ref.into(),
            resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
            tree_hash: status.tree_hash.clone().unwrap_or_default(),
            path_filters: merged_filters(&status.path_filters, &selector.path_filters),
            language_filters: merged_filters(&status.language_filters, &selector.language_filters),
            index_versions: vec![format!(
                "code:{}",
                status.tree_hash.as_deref().unwrap_or("unindexed")
            )],
            stale: status.stale,
        }
    }
}

/// Code repository retrieval response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryQueryResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub request: CodeRetrievalRequest,
    pub results: Vec<CodeRetrievalHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Code repository impact response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryImpactResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub request: CodeImpactRequest,
    pub changed_paths: Vec<String>,
    pub results: Vec<CodeRetrievalHit>,
}

/// Code repository status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryStatusResponse {
    pub metadata: ApiMetadata,
    pub status: CodeRepositoryStatus,
}

fn merged_filters(base: &[String], request: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in base.iter().chain(request.iter()) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }

    merged
}
