//! Stable API contracts shared by CLI, Web, and future service adapters.

mod agent;
mod code_repository;
mod context;
mod error;
mod file_index;
mod metadata;
mod operations;
mod service_plan;
mod status;
mod stream;
mod watcher_diagnostics;

pub use agent::{
    AgentAccessPolicy, AgentAccessPolicySummary, AgentBudgetUsed, AgentPolicyError,
    AgentProtocolKind, AgentProtocolStatus, AgentRequestContext, AgentRetrievalResult,
    RuntimeIdentity, freshness_label,
};
pub(crate) use code_repository::CodeRepositoryFreshnessInput;
pub use code_repository::{
    CodeGraphContextResponse, CodeRepositoryFreshnessCursor, CodeRepositoryFreshnessDiagnostics,
    CodeRepositoryFreshnessState, CodeRepositoryIndexLag, CodeRepositoryPendingIndexWork,
};
pub use context::{InterfaceKind, RequestContext};
pub use error::{ApiError, ErrorKind};
pub use file_index::{
    FileContentQueryRequest, FileContentQueryResponse, FileIndexFreshnessCursor,
    FileIndexFreshnessDiagnostics, FileIndexFreshnessState, FileIndexLag, FileIndexRequest,
    FileIndexResponse, FileQueryRequest, FileQueryResponse,
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
    GRAPH_CANVAS_DEFAULT_LIMIT, GRAPH_CANVAS_MAX_LIMIT, GraphCanvasEdge, GraphCanvasKind,
    GraphCanvasNode, GraphCanvasRequest, GraphCanvasResponse, GraphCanvasSummary,
    GraphInspectionRequest, GraphInspectionResponse, HealthResponse, HybridRetrievalRequest,
    HybridRetrievalResponse, IndexRefreshRequest, IndexRefreshResponse, IngestClaim, IngestEvent,
    IngestEvidence, IngestEvidenceExtraction, IngestRelation, IngestRequest, IngestResponse,
    MultimodalExtractionRequest, MultimodalExtractionResponse, ProposalDecisionApiRequest,
    ProposalDecisionResponse, ProposalListApiRequest, ProposalListResponse, ProposalShowResponse,
    ServiceDefinitionWriteResponse, ServiceOperatorResponse, ServiceRecoveryReport,
    ServiceStatusResponse, SoftwareGlobalResponse, StorageShardDiagnostics,
    StorageTopologyDiagnostics, StorageTopologyResponse, WorkerRunRequest, WorkerRunResponse,
    WorkerStatusRequest, WorkerStatusResponse,
};
pub use service_plan::{ServicePlanRequest, ServicePlanResponse};
pub use status::{ProjectStatusResponse, RuntimeStatus};
pub use stream::{ApiStreamEvent, StreamEventKind};
pub use watcher_diagnostics::WatcherDiagnostics;
