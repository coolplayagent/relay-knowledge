use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeImpactPathGroups, CodeImpactRequest,
        CodeIndexCheckpoint, CodeIndexSummary, CodeIndexTaskRecord, CodeRepositoryRegistration,
        CodeRepositoryReport, CodeRepositoryScopePreview, CodeRepositorySelector,
        CodeRepositorySet, CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
        CodeRepositorySetMember, CodeRepositorySetQueryHit, CodeRepositorySetQueryRequest,
        CodeRepositorySetRefreshSummary, CodeRepositorySetRefreshTaskRecord,
        CodeRepositorySetRemoveMemberRequest, CodeRepositorySetStatus, CodeRepositoryStatus,
        CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest, CodeScopeRetentionSummary,
        CommitReceipt, ConfidenceScore, EvidenceExtractionMetadata, EvidenceModality, EvidenceSpan,
        ExtractionDiagnostic, FactStatus, FreshnessPolicy, FusionDiagnostics, GraphVersionRange,
        IndexKind, IndexStatus, LayoutRegion, ProposalConflictRecord, ProposalRecord,
        ProposalState, RerankDiagnostics, RetrievalBackendStatus, RetrievalBudgetUsed,
        RetrievalHit, RetrievalMode, RetrievedContextPack, ServiceDefinitionPlan,
        ServiceManagerAction, ServiceOperatorStatus, SoftwareComponent, SoftwareDependencyUsage,
        SoftwareGlobalRequest, SoftwareGlobalStatus, SoftwareSdkUsage, WorkerKind, WorkerStatus,
        WorkerTaskRecord,
    },
    storage::{GraphInspection, IndexCursor, IndexRefreshDiagnostics},
};

use super::{AgentProtocolStatus, ApiMetadata, RuntimeStatus};

pub const GRAPH_CANVAS_DEFAULT_LIMIT: usize = 250;
pub const GRAPH_CANVAS_MAX_LIMIT: usize = 1000;

/// Graph canvas view selected by the Web workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphCanvasKind {
    Knowledge,
    Code,
    Mixed,
}

impl GraphCanvasKind {
    /// Parses the stable Web query representation.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "knowledge" => Ok(Self::Knowledge),
            "code" => Ok(Self::Code),
            "mixed" => Ok(Self::Mixed),
            _ => Err(format!("unsupported graph canvas kind '{value}'")),
        }
    }

    /// Returns the stable Web query representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge",
            Self::Code => "code",
            Self::Mixed => "mixed",
        }
    }
}

/// Bounded graph canvas request for same-origin Web exploration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasRequest {
    pub kind: GraphCanvasKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub limit: usize,
}

/// Node rendered in the Web graph canvas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub graph_version: u64,
    pub weight: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

/// Edge rendered in the Web graph canvas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasEdge {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub target: String,
    pub label: String,
    pub graph_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_basis_points: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_count: Option<usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

/// Bounded graph canvas summary for truncation and legend hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasSummary {
    pub kind: GraphCanvasKind,
    pub node_count: usize,
    pub edge_count: usize,
    pub truncated: bool,
    pub available_kinds: Vec<String>,
}

/// Same-origin graph canvas response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCanvasResponse {
    pub metadata: ApiMetadata,
    pub nodes: Vec<GraphCanvasNode>,
    pub edges: Vec<GraphCanvasEdge>,
    pub summary: GraphCanvasSummary,
}

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
    pub rerank: RerankDiagnostics,
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
    pub repository_code_totals: CodeRepositoryTotals,
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

/// Bounded local file indexing request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(default)]
    pub roots: Vec<String>,
}

/// Local file indexing response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexResponse {
    pub metadata: ApiMetadata,
    pub summary: crate::storage::FileIndexScanSummary,
}

/// Bounded local file-location query request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileQueryRequest {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    pub limit: usize,
}

/// Local file-location query response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileQueryResponse {
    pub metadata: ApiMetadata,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    pub results: Vec<crate::storage::FileSearchHit>,
    pub truncated: bool,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
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
    pub file_index: crate::storage::FileIndexDiagnostics,
    pub agent_protocols: AgentProtocolStatus,
    pub operator: ServiceOperatorStatus,
    pub workers: Vec<WorkerStatus>,
    pub proposal_backlog: usize,
    pub audit_sink: AuditSinkStatus,
}

/// Durable audit sink health surfaced in service diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSinkStatus {
    pub durable: bool,
    pub event_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Worker status filter. Missing kind means all worker families.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatusRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<WorkerKind>,
}

/// Worker status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatusResponse {
    pub metadata: ApiMetadata,
    pub workers: Vec<WorkerStatus>,
}

/// Bounded foreground worker run request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRunRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<WorkerKind>,
}

/// Bounded foreground worker run response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRunResponse {
    pub metadata: ApiMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<WorkerTaskRecord>,
    #[serde(default)]
    pub proposals: Vec<ProposalRecord>,
    pub workers: Vec<WorkerStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Proposal list filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalListApiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ProposalState>,
    pub limit: usize,
}

/// Proposal list response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalListResponse {
    pub metadata: ApiMetadata,
    pub proposals: Vec<ProposalRecord>,
}

/// Proposal detail response with conflict lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalShowResponse {
    pub metadata: ApiMetadata,
    pub proposal: ProposalRecord,
    pub conflicts: Vec<ProposalConflictRecord>,
    pub payload: serde_json::Value,
}

/// Manual proposal decision request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDecisionApiRequest {
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Manual proposal decision response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDecisionResponse {
    pub metadata: ApiMetadata,
    pub proposal: ProposalRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<CommitReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_refresh_error: Option<String>,
}

/// Durable audit query request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditQueryApiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub limit: usize,
}

/// Durable audit query response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditQueryResponse {
    pub metadata: ApiMetadata,
    pub events: Vec<crate::domain::AuditEventRecord>,
}

/// Service manager plan request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePlanRequest {
    pub action: ServiceManagerAction,
}

/// Service manager plan response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePlanResponse {
    pub metadata: ApiMetadata,
    pub plan: ServiceDefinitionPlan,
}

/// Service definition write response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceDefinitionWriteResponse {
    pub metadata: ApiMetadata,
    pub plan: ServiceDefinitionPlan,
    pub written: bool,
}

/// Service silent-update operator response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceOperatorResponse {
    pub metadata: ApiMetadata,
    pub operator: ServiceOperatorStatus,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub graph: GraphInspection,
    pub repository_code_totals: CodeRepositoryTotals,
    pub indexes: Vec<IndexStatus>,
    pub index_cursors: Vec<IndexCursor>,
    pub index_refresh: IndexRefreshDiagnostics,
    pub file_index: crate::storage::FileIndexDiagnostics,
    pub runtime: RuntimeStatus,
}

/// Remote embedding provider probe response with secret-free diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingProviderProbeResponse {
    pub metadata: ApiMetadata,
    pub ok: bool,
    pub provider: Option<String>,
    pub model: String,
    pub dimension: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}

/// Code repository registration request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRegisterRequest {
    pub root_path: String,
    #[serde(default)]
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

/// Code repository index start response for queued or no-op index requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryIndexStartResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<CodeIndexSummary>,
    pub status: CodeRepositoryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<CodeIndexTaskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<CodeIndexCheckpoint>,
}

/// Code repository scope preview response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryScopePreviewResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub preview: CodeRepositoryScopePreview,
}

/// Code repository scope and index metadata attached to code responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryScopeMetadata {
    pub scope_id: String,
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
            scope_id: status.last_indexed_scope_id.clone().unwrap_or_default(),
            repository_id: status.repository_id.clone(),
            alias: status.alias.clone(),
            requested_ref: requested_ref.into(),
            resolved_commit_sha: status.last_indexed_commit.clone().unwrap_or_default(),
            tree_hash: status.tree_hash.clone().unwrap_or_default(),
            path_filters: merged_filters(&status.path_filters, &selector.path_filters),
            language_filters: merged_filters(&status.language_filters, &selector.language_filters),
            index_versions: vec![format!(
                "code:{}:{}",
                status
                    .last_indexed_scope_id
                    .as_deref()
                    .unwrap_or("unscoped"),
                status.tree_hash.as_deref().unwrap_or("unindexed")
            )],
            stale: status.stale,
        }
    }

    /// Builds scope metadata for a queued or running index task.
    pub fn from_index_task(task: &CodeIndexTaskRecord, requested_ref: impl Into<String>) -> Self {
        Self {
            scope_id: task.source_scope.clone(),
            repository_id: task.repository_id.clone(),
            alias: task.alias.clone(),
            requested_ref: requested_ref.into(),
            resolved_commit_sha: task.resolved_commit_sha.clone(),
            tree_hash: task.tree_hash.clone(),
            path_filters: task.path_filters.clone(),
            language_filters: task.language_filters.clone(),
            index_versions: vec![format!("code:{}:{}", task.source_scope, task.tree_hash)],
            stale: true,
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

/// Code repository feature-flag graph response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryFeatureFlagsResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub request: CodeFeatureFlagRequest,
    pub flags: Vec<CodeFeatureFlagGraph>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Code repository impact response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryImpactResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub request: CodeImpactRequest,
    pub path_groups: CodeImpactPathGroups,
    pub results: Vec<CodeRetrievalHit>,
}

/// Code repository status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryStatusResponse {
    pub metadata: ApiMetadata,
    pub status: CodeRepositoryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task: Option<CodeIndexTaskRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<CodeIndexCheckpoint>,
    pub retention: CodeScopeRetentionSummary,
}

/// Code repository operations report response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositoryReportResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub report: CodeRepositoryReport,
}

/// Repository-scoped software global model projection response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareGlobalResponse {
    pub metadata: ApiMetadata,
    pub scope: CodeRepositoryScopeMetadata,
    pub request: SoftwareGlobalRequest,
    pub status: SoftwareGlobalStatus,
    pub components: Vec<SoftwareComponent>,
    pub dependency_usages: Vec<SoftwareDependencyUsage>,
    pub sdk_usages: Vec<SoftwareSdkUsage>,
}

/// Repository-set creation response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetCreateResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRepositorySetCreateRequest,
    pub repository_set: CodeRepositorySet,
}

/// Repository-set member addition response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetAddResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRepositorySetAddMemberRequest,
    pub member: CodeRepositorySetMember,
    pub status: CodeRepositorySetStatus,
}

/// Repository-set member removal response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetRemoveResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRepositorySetRemoveMemberRequest,
    pub member: CodeRepositorySetMember,
    pub status: CodeRepositorySetStatus,
}

/// Repository-set query response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositorySetQueryResponse {
    pub metadata: ApiMetadata,
    pub request: CodeRepositorySetQueryRequest,
    pub status: CodeRepositorySetStatus,
    pub results: Vec<CodeRepositorySetQueryHit>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Repository-set status response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetStatusResponse {
    pub metadata: ApiMetadata,
    pub status: CodeRepositorySetStatus,
}

/// Repository-set overlay refresh response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetRefreshResponse {
    pub metadata: ApiMetadata,
    pub status: CodeRepositorySetStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<CodeRepositorySetRefreshSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<CodeRepositorySetRefreshTaskRecord>,
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
