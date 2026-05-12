//! Agent protocol adapters for resident relay-knowledge processes.

pub mod mcp;
mod policy;

pub use policy::{AgentAdapterError, AgentAdapterErrorKind, authorize_limit, authorize_scope};
