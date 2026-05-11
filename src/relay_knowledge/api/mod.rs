//! Stable API contracts shared by CLI, Web, and future service adapters.

mod context;
mod error;
mod metadata;
mod status;
mod stream;

pub use context::{InterfaceKind, RequestContext};
pub use error::{ApiError, ErrorKind};
pub use metadata::ApiMetadata;
pub use status::{ProjectStatusResponse, RuntimeStatus};
pub use stream::{ApiStreamEvent, StreamEventKind};
