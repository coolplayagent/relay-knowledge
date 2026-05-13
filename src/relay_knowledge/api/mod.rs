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
    CodeRepositoryImpactResponse, CodeRepositoryIndexResponse, CodeRepositoryQueryResponse,
    CodeRepositoryRegisterRequest, CodeRepositoryRegisterResponse, CodeRepositoryReportResponse,
    CodeRepositoryScopeMetadata, CodeRepositoryScopePreviewResponse, CodeRepositoryStatusResponse,
    EmbeddingProviderProbeResponse, GraphInspectionRequest, GraphInspectionResponse,
    HealthResponse, HybridRetrievalRequest, HybridRetrievalResponse, IndexRefreshRequest,
    IndexRefreshResponse, IngestClaim, IngestEvent, IngestEvidence, IngestEvidenceExtraction,
    IngestRelation, IngestRequest, IngestResponse, MultimodalExtractionRequest,
    MultimodalExtractionResponse, ServiceRecoveryReport, ServiceStatusResponse,
};
pub use status::{ProjectStatusResponse, RuntimeStatus};
pub use stream::{ApiStreamEvent, StreamEventKind};
