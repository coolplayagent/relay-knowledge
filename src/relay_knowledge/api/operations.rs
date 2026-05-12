use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        CodeImpactRequest, CodeIndexSummary, CodeRepositoryRegistration, CodeRepositoryStatus,
        CodeRetrievalHit, CodeRetrievalRequest, CommitReceipt, FreshnessPolicy, FusionDiagnostics,
        IndexKind, IndexStatus, RetrievalBudgetUsed, RetrievalHit, RetrievalMode,
        RetrievedContextPack,
    },
    storage::GraphInspection,
};

use super::{ApiMetadata, RuntimeStatus};

/// Evidence item supplied to the ingest API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestEvidence {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub content: String,
    #[serde(default)]
    pub entity_labels: Vec<String>,
}

/// Ingest request shared by CLI, Web, HTTP, and future agent adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestRequest {
    pub source_scope: String,
    pub evidence: Vec<IngestEvidence>,
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
}

/// Aggregated health response for CLI/Web/service diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub metadata: ApiMetadata,
    pub healthy: bool,
    pub graph: GraphInspection,
    pub indexes: Vec<IndexStatus>,
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
    pub summary: CodeIndexSummary,
    pub status: CodeRepositoryStatus,
}

/// Code repository retrieval response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryQueryResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRetrievalRequest,
    pub results: Vec<CodeRetrievalHit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Code repository impact response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryImpactResponse {
    pub metadata: ApiMetadata,
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
