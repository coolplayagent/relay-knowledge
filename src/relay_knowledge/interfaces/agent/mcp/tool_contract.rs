use std::fmt;

use serde_json::{Value, json};

use crate::{
    api::{ApiError, ErrorKind, InterfaceKind, RequestContext},
    domain::FreshnessPolicy,
};

use super::{AgentAdapterError, AgentAdapterErrorKind};

pub(super) fn parse_freshness(value: Option<&str>) -> Result<FreshnessPolicy, AgentAdapterError> {
    match value.unwrap_or("allow-stale") {
        "allow-stale" => Ok(FreshnessPolicy::AllowStale),
        "wait-until-fresh" => Ok(FreshnessPolicy::WaitUntilFresh),
        "graph-only" => Ok(FreshnessPolicy::GraphOnly),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid freshness '{other}'"),
        )),
    }
}

pub(super) fn tool_success_result(summary: impl Into<String>, structured_content: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": summary.into()}],
        "structuredContent": structured_content,
        "isError": false
    })
}

pub(super) fn api_error_result(error: ApiError) -> Value {
    tool_error_result(AgentAdapterError::new(
        agent_error_kind(error.error_kind),
        error.message,
    ))
}

fn agent_error_kind(kind: ErrorKind) -> AgentAdapterErrorKind {
    match kind {
        ErrorKind::InvalidArgument => AgentAdapterErrorKind::InvalidArgument,
        ErrorKind::StorageUnavailable => AgentAdapterErrorKind::StorageUnavailable,
        ErrorKind::QosRejected => AgentAdapterErrorKind::QosRejected,
        ErrorKind::Timeout => AgentAdapterErrorKind::Timeout,
        ErrorKind::Internal => AgentAdapterErrorKind::Internal,
    }
}

pub(super) fn tool_error_result(error: AgentAdapterError) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": format!("{}: {}", error.kind.as_str(), error.message)
        }],
        "structuredContent": {
            "error_kind": error.kind.as_str(),
            "message": error.message,
        },
        "isError": true
    })
}

pub(super) fn invalid_arguments(error: serde_json::Error) -> AgentAdapterError {
    AgentAdapterError::new(
        AgentAdapterErrorKind::InvalidArgument,
        format!("invalid tool arguments: {error}"),
    )
}

pub(super) fn domain_argument_error(error: impl fmt::Display) -> AgentAdapterError {
    AgentAdapterError::new(AgentAdapterErrorKind::InvalidArgument, error.to_string())
}

pub(super) fn request_context(request_id: String) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Mcp,
        format!("mcp-{request_id}"),
        format!("trace-mcp-{request_id}"),
    )
}

#[cfg(test)]
#[path = "tool_contract_tests.rs"]
mod tests;
