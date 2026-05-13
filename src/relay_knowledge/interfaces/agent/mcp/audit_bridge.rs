use serde_json::Value;

use crate::{
    api::{AgentProtocolKind, RuntimeIdentity},
    interfaces::agent::{AgentAuditQosDecision, AgentAuditStatus},
};

use super::{AgentAuditEvent, McpServer, request_id_key};

pub(super) fn record_mcp_qos_rejection(
    server: &McpServer,
    operation: &str,
    id: &Value,
    error_kind: &str,
) {
    let request_id = request_id_key("mcp", id).unwrap_or_else(|| "mcp|invalid-id".to_owned());
    server.audit.record(AgentAuditEvent {
        sequence: 0,
        protocol: AgentProtocolKind::Mcp,
        operation: operation.to_owned(),
        request_id: request_id.clone(),
        trace_id: format!("trace-{request_id}"),
        runtime_identity: RuntimeIdentity::mcp(Some(request_id)),
        qos_decision: AgentAuditQosDecision::Rejected,
        status: AgentAuditStatus::Failed,
        source_scope: None,
        freshness: None,
        limit: None,
        result_count: None,
        truncated: false,
        elapsed_ms: 0,
        error_kind: Some(error_kind.to_owned()),
    });
}

pub(super) fn record_mcp_tool_audit(
    server: &McpServer,
    operation: &str,
    request_id: &str,
    result: &Value,
    elapsed_ms: u64,
) {
    let structured = &result["structuredContent"];
    let error_kind = structured["error_kind"].as_str().map(str::to_owned);
    let is_error = result["isError"].as_bool().unwrap_or(false);
    let status = match error_kind.as_deref() {
        Some("cancelled") => AgentAuditStatus::Cancelled,
        _ if is_error => AgentAuditStatus::Failed,
        _ => AgentAuditStatus::Completed,
    };

    server.audit.record(AgentAuditEvent {
        sequence: 0,
        protocol: AgentProtocolKind::Mcp,
        operation: operation.to_owned(),
        request_id: request_id.to_owned(),
        trace_id: format!("trace-mcp-{request_id}"),
        runtime_identity: RuntimeIdentity::mcp(Some(request_id.to_owned())),
        qos_decision: AgentAuditQosDecision::Admitted,
        status,
        source_scope: audit_source_scope(structured),
        freshness: structured["freshness"].as_str().map(str::to_owned),
        limit: structured["budget_used"]["limit"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok()),
        result_count: audit_result_count(structured),
        truncated: structured["truncated"].as_bool().unwrap_or(false),
        elapsed_ms,
        error_kind,
    });
}

pub(super) struct McpMethodAudit<'a> {
    pub(super) operation: &'a str,
    pub(super) request_id: &'a str,
    pub(super) status: AgentAuditStatus,
    pub(super) source_scope: Option<String>,
    pub(super) result_count: Option<usize>,
    pub(super) elapsed_ms: u64,
    pub(super) error_kind: Option<&'a str>,
}

pub(super) fn record_mcp_method_audit(server: &McpServer, input: McpMethodAudit<'_>) {
    server.audit.record(AgentAuditEvent {
        sequence: 0,
        protocol: AgentProtocolKind::Mcp,
        operation: input.operation.to_owned(),
        request_id: input.request_id.to_owned(),
        trace_id: format!("trace-mcp-{}", input.request_id),
        runtime_identity: RuntimeIdentity::mcp(Some(input.request_id.to_owned())),
        qos_decision: AgentAuditQosDecision::Admitted,
        status: input.status,
        source_scope: input.source_scope,
        freshness: None,
        limit: None,
        result_count: input.result_count,
        truncated: false,
        elapsed_ms: input.elapsed_ms,
        error_kind: input.error_kind.map(str::to_owned),
    });
}

fn audit_source_scope(structured: &Value) -> Option<String> {
    structured["source_scope"]
        .as_str()
        .or_else(|| structured["request"]["repository"]["repository"].as_str())
        .map(str::to_owned)
}

fn audit_result_count(structured: &Value) -> Option<usize> {
    if let Some(returned) = structured["budget_used"]["returned_count"].as_u64() {
        return usize::try_from(returned).ok();
    }

    structured["results"].as_array().map(Vec::len)
}
