//! Authored research contracts. Assertions never constitute verified approval.

mod bundle;
mod catalog;
mod requirements;
pub use bundle::*;
pub use catalog::*;
pub use requirements::*;

/// Repository target freshness is independent from source review and capture integrity.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResearchRepositoryState {
    pub state: String,
    pub requested_ref: String,
    pub resolved_target: Option<String>,
    pub registration: Option<crate::domain::CodeRepositoryStatus>,
    pub served_scope: Option<crate::domain::CodeRepositoryStatus>,
    pub diagnostic: Option<String>,
}
