//! Application services that orchestrate domain behavior behind stable API types.

// `ApiError` is a stable 1.x public DTO whose optional metadata keeps the JSON and Rust API
// layouts compatible. Boxing that field would be a breaking Rust API change, so application
// boundary methods intentionally return the existing error shape until the 2.0 contract cleanup.
#![allow(clippy::result_large_err)]

mod code_repository;
mod knowledge;
mod model_provider;
mod runtime;
mod service;
mod update;
mod worker;

pub use knowledge::DEFAULT_FILE_QUERY_LIMIT;
pub use knowledge::map::KnowledgeMapSourceAddRequest;
pub(crate) use knowledge::map::{
    KnowledgeMapService, KnowledgeMapServiceError, MAX_HISTORY_PAGE_SIZE,
};
pub use runtime::{
    AgentRuntimeConfig, FileIndexRootConfig, FileIndexRuntimeConfig, ProcessRuntimeConfig,
    RetrievalRuntimeConfigError, RuntimeConfiguration, RuntimeConfigurationError,
    WorkerRuntimeConfig,
};
pub use service::{AgentDurableAuditInput, RelayKnowledgeService};
pub use update::{
    UpdateRuntimeConfig, UpdateRuntimeConfigError, UpdateSource, VersionCheckResponse,
    update_notice,
};
