//! Agent protocol adapters for resident relay-knowledge processes.

pub mod acp;
mod audit;
pub mod mcp;
mod policy;

pub use audit::{
    AgentAuditEvent, AgentAuditLog, AgentAuditQosDecision, AgentAuditSink, AgentAuditStatus,
};
pub use policy::{
    AgentAdapterError, AgentAdapterErrorKind, MAX_AGENT_PATH_CHARS, MAX_AGENT_QUERY_CHARS,
    authorize_limit, authorize_scope, normalize_scope_for_policy, scope_not_authorized,
    validate_optional_query_text, validate_path_texts, validate_query_text,
};
