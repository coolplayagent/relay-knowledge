//! Application services that orchestrate domain behavior behind stable API types.

mod code_query_source_fallback;
mod code_query_source_surface;
mod code_repository_set_identity_coverage;
mod code_repository_set_lifecycle;
mod code_repository_set_plan;
mod code_repository_set_query;
mod code_repository_set_service;
mod code_service;
mod file_index;
mod index_refresh;
mod ingest;
mod model_provider_config;
mod multimodal;
mod operations;
mod runtime;
mod service;
mod status;
mod update;
mod worker_proposals;

pub use file_index::DEFAULT_FILE_QUERY_LIMIT;
pub use runtime::{
    AgentRuntimeConfig, FileIndexRootConfig, FileIndexRuntimeConfig, RetrievalRuntimeConfigError,
    RuntimeConfiguration, RuntimeConfigurationError, WorkerRuntimeConfig,
};
pub use service::{AgentDurableAuditInput, RelayKnowledgeService};
pub use update::{
    UpdateRuntimeConfig, UpdateRuntimeConfigError, UpdateSource, VersionCheckResponse,
    update_notice,
};
