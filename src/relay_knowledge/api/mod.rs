//! Stable API contracts shared by CLI, Web, and future service adapters.

mod contracts;
mod operations;

pub(crate) use contracts::CodeRepositoryFreshnessInput;
pub use contracts::*;
pub use operations::{
    AuditQueryApiRequest, AuditQueryResponse, AuditSinkStatus, BusinessKnowledgeQueryResponse,
    CodeIndexWorkerRunRequest, CodeIndexWorkerRunResponse, CodeIndexWorkerStatus,
    CodeRepositoryFeatureFlagsResponse, CodeRepositoryFrameworkGraphResponse,
    CodeRepositoryImpactResponse, CodeRepositoryIndexResetResponse, CodeRepositoryIndexResponse,
    CodeRepositoryIndexStartResponse, CodeRepositoryListResponse, CodeRepositoryQueryResponse,
    CodeRepositoryRegisterRequest, CodeRepositoryRegisterResponse, CodeRepositoryRemoveResponse,
    CodeRepositoryReportResponse, CodeRepositoryScopePreviewResponse, CodeRepositorySetAddResponse,
    CodeRepositorySetCreateResponse, CodeRepositorySetQueryResponse,
    CodeRepositorySetRefreshResponse, CodeRepositorySetRemoveResponse,
    CodeRepositorySetStatusResponse, CodeRepositoryStatusResponse, CodeRepositoryUpdateRequest,
    EmbeddingProviderProbeResponse, GRAPH_CANVAS_DEFAULT_LIMIT, GRAPH_CANVAS_MAX_LIMIT,
    GraphCanvasEdge, GraphCanvasKind, GraphCanvasNode, GraphCanvasRequest, GraphCanvasResponse,
    GraphCanvasSummary, GraphInspectionRequest, GraphInspectionResponse, HealthResponse,
    HybridRetrievalRequest, HybridRetrievalResponse, IndexRefreshRequest, IndexRefreshResponse,
    IngestClaim, IngestEvent, IngestEvidence, IngestEvidenceExtraction, IngestRelation,
    IngestRequest, IngestResponse, MultimodalExtractionRequest, MultimodalExtractionResponse,
    ProposalDecisionApiRequest, ProposalDecisionResponse, ProposalListApiRequest,
    ProposalListResponse, ProposalShowResponse, RepositoryGraphNeighborhoodResponseV1,
    ServiceDefinitionWriteResponse, ServiceOperatorResponse, ServiceRecoveryReport,
    ServiceStatusResponse, SoftwareGlobalExportResponse, SoftwareGlobalResponse,
    StorageShardDiagnostics, StorageTopologyDiagnostics, StorageTopologyResponse, WorkerRunRequest,
    WorkerRunResponse, WorkerStatusRequest, WorkerStatusResponse,
};
