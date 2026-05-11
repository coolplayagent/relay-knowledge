//! Stable API contracts shared by CLI, Web, and future service adapters.

mod context;
mod error;
mod metadata;
mod operations;
mod status;
mod stream;

pub use context::{InterfaceKind, RequestContext};
pub use error::{ApiError, ErrorKind};
pub use metadata::ApiMetadata;
pub use operations::{
    GraphInspectionRequest, GraphInspectionResponse, HealthResponse, HybridRetrievalRequest,
    HybridRetrievalResponse, IndexRefreshRequest, IndexRefreshResponse, IngestEvidence,
    IngestRequest, IngestResponse, ServiceStatusResponse,
};
pub use status::{ProjectStatusResponse, RuntimeStatus};
pub use stream::{ApiStreamEvent, StreamEventKind};
