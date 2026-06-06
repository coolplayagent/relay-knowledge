//! Stable API contracts shared by CLI, Web, and future service adapters.

mod agent;
mod code_repository;
mod context;
mod error;
mod file_index;
mod metadata;
mod operations;
mod status;
mod stream;

pub use agent::{
    AgentAccessPolicy, AgentAccessPolicySummary, AgentBudgetUsed, AgentPolicyError,
    AgentProtocolKind, AgentProtocolStatus, AgentRequestContext, AgentRetrievalResult,
    RuntimeIdentity, freshness_label,
};
pub(crate) use code_repository::CodeRepositoryFreshnessInput;
pub use code_repository::{
    CodeRepositoryFreshnessCursor, CodeRepositoryFreshnessDiagnostics,
    CodeRepositoryFreshnessState, CodeRepositoryIndexLag, CodeRepositoryPendingIndexWork,
};
pub use context::{InterfaceKind, RequestContext};
pub use error::{ApiError, ErrorKind};
pub use file_index::{
    FileIndexFreshnessCursor, FileIndexFreshnessDiagnostics, FileIndexFreshnessState, FileIndexLag,
};
pub use metadata::ApiMetadata;
pub use operations::{
    AuditQueryApiRequest, AuditQueryResponse, AuditSinkStatus, CodeIndexWorkerRunRequest,
    CodeIndexWorkerRunResponse, CodeIndexWorkerStatus, CodeRepositoryFeatureFlagsResponse,
    CodeRepositoryImpactResponse, CodeRepositoryIndexResetResponse, CodeRepositoryIndexResponse,
    CodeRepositoryIndexStartResponse, CodeRepositoryQueryResponse, CodeRepositoryRegisterRequest,
    CodeRepositoryRegisterResponse, CodeRepositoryRemoveResponse, CodeRepositoryReportResponse,
    CodeRepositoryScopeMetadata, CodeRepositoryScopePreviewResponse, CodeRepositorySetAddResponse,
    CodeRepositorySetCreateResponse, CodeRepositorySetQueryResponse,
    CodeRepositorySetRefreshResponse, CodeRepositorySetRemoveResponse,
    CodeRepositorySetStatusResponse, CodeRepositoryStatusResponse, EmbeddingProviderProbeResponse,
    FileIndexRequest, FileIndexResponse, FileQueryRequest, FileQueryResponse,
    GRAPH_CANVAS_DEFAULT_LIMIT, GRAPH_CANVAS_MAX_LIMIT, GraphCanvasEdge, GraphCanvasKind,
    GraphCanvasNode, GraphCanvasRequest, GraphCanvasResponse, GraphCanvasSummary,
    GraphInspectionRequest, GraphInspectionResponse, HealthResponse, HybridRetrievalRequest,
    HybridRetrievalResponse, IndexRefreshRequest, IndexRefreshResponse, IngestClaim, IngestEvent,
    IngestEvidence, IngestEvidenceExtraction, IngestRelation, IngestRequest, IngestResponse,
    MultimodalExtractionRequest, MultimodalExtractionResponse, ProposalDecisionApiRequest,
    ProposalDecisionResponse, ProposalListApiRequest, ProposalListResponse, ProposalShowResponse,
    ServiceDefinitionWriteResponse, ServiceOperatorResponse, ServicePlanRequest,
    ServicePlanResponse, ServiceRecoveryReport, ServiceStatusResponse, SoftwareGlobalResponse,
    StorageShardDiagnostics, StorageTopologyDiagnostics, StorageTopologyResponse, WorkerRunRequest,
    WorkerRunResponse, WorkerStatusRequest, WorkerStatusResponse,
};
pub use status::{ProjectStatusResponse, RuntimeStatus};
pub use stream::{ApiStreamEvent, StreamEventKind};
