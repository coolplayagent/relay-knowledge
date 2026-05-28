//! Application services that orchestrate domain behavior behind stable API types.

mod code_repository;
mod knowledge;
mod model_provider;
mod runtime;
mod service;
mod status;
mod update;
mod worker;

pub use knowledge::DEFAULT_FILE_QUERY_LIMIT;
pub use knowledge::map::KnowledgeMapSourceAddRequest;
pub(crate) use knowledge::map::{KnowledgeMapService, KnowledgeMapServiceError};
pub use runtime::{
    AgentRuntimeConfig, FileIndexRootConfig, FileIndexRuntimeConfig, RetrievalRuntimeConfigError,
    RuntimeConfiguration, RuntimeConfigurationError, WorkerRuntimeConfig,
};
pub(crate) use service::knowledge_map::knowledge_map_service;
pub use service::{AgentDurableAuditInput, RelayKnowledgeService};
pub use update::{
    UpdateRuntimeConfig, UpdateRuntimeConfigError, UpdateSource, VersionCheckResponse,
    update_notice,
};
