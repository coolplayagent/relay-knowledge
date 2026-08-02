//! Stable request, response, status, and streaming contracts.

mod agent;
mod code_repository;
mod codebase_views;
mod context;
mod error;
mod file_index;
mod metadata;
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
    CodeRepositoryScopeMetadata,
};
pub use codebase_views::CodebaseViewResponse;
pub use context::{InterfaceKind, RequestContext};
pub use error::{ApiError, ErrorKind};
pub use file_index::{
    FileContentQueryRequest, FileContentQueryResponse, FileIndexFreshnessCursor,
    FileIndexFreshnessDiagnostics, FileIndexFreshnessState, FileIndexLag, FileIndexRequest,
    FileIndexResponse, FileQueryRequest, FileQueryResponse,
};
pub use metadata::ApiMetadata;
pub use service_plan::{ServicePlanRequest, ServicePlanResponse};
pub use status::{ProjectStatusResponse, RuntimeStatus};
pub use stream::{ApiStreamEvent, StreamEventKind};
pub use watcher_diagnostics::WatcherDiagnostics;
