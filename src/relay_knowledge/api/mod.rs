//! Stable API contracts shared by CLI, Web, and future service adapters.

mod agent;
mod context;
mod error;
mod metadata;
mod operations;
mod status;
mod stream;

pub use agent::{
    AgentAccessPolicy, AgentAccessPolicySummary, AgentBudgetUsed, AgentPolicyError,
    AgentProtocolKind, AgentProtocolStatus, AgentRequestContext, AgentRetrievalResult,
    RuntimeIdentity, freshness_label,
};
pub use context::{InterfaceKind, RequestContext};
pub use error::{ApiError, ErrorKind};
pub use metadata::ApiMetadata;
pub use operations::{
    AuditQueryApiRequest, AuditQueryResponse, AuditSinkStatus, CodeRepositoryImpactResponse,
    CodeRepositoryIndexResponse, CodeRepositoryIndexStartResponse, CodeRepositoryQueryResponse,
    CodeRepositoryRegisterRequest, CodeRepositoryRegisterResponse, CodeRepositoryReportResponse,
    CodeRepositoryScopeMetadata, CodeRepositoryScopePreviewResponse, CodeRepositorySetAddResponse,
    CodeRepositorySetCreateResponse, CodeRepositorySetQueryResponse,
    CodeRepositorySetRefreshResponse, CodeRepositorySetStatusResponse,
    CodeRepositoryStatusResponse, EmbeddingProviderProbeResponse, FileIndexRequest,
    FileIndexResponse, FileQueryRequest, FileQueryResponse, GRAPH_CANVAS_DEFAULT_LIMIT,
    GRAPH_CANVAS_MAX_LIMIT, GraphCanvasEdge, GraphCanvasKind, GraphCanvasNode, GraphCanvasRequest,
    GraphCanvasResponse, GraphCanvasSummary, GraphInspectionRequest, GraphInspectionResponse,
    HealthResponse, HybridRetrievalRequest, HybridRetrievalResponse, IndexRefreshRequest,
    IndexRefreshResponse, IngestClaim, IngestEvent, IngestEvidence, IngestEvidenceExtraction,
    IngestRelation, IngestRequest, IngestResponse, MultimodalExtractionRequest,
    MultimodalExtractionResponse, ProposalDecisionApiRequest, ProposalDecisionResponse,
    ProposalListApiRequest, ProposalListResponse, ProposalShowResponse,
    ServiceDefinitionWriteResponse, ServiceOperatorResponse, ServicePlanRequest,
    ServicePlanResponse, ServiceRecoveryReport, ServiceStatusResponse, WorkerRunRequest,
    WorkerRunResponse, WorkerStatusRequest, WorkerStatusResponse,
};
pub use status::{ProjectStatusResponse, RuntimeStatus};
pub use stream::{ApiStreamEvent, StreamEventKind};
