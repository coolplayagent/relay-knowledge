use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::interfaces::agent::{AgentAdapterError, AgentAdapterErrorKind};

use super::super::{
    code_tools::run_code_tool,
    tool_contract::tool_error_result,
    tool_registry::{
        CODE_CONTEXT_TOOL, CODE_FEATURE_FLAGS_TOOL, CODE_IMPACT_TOOL, CODE_QUERY_TOOL,
        CODE_REPOSITORY_SET_QUERY_TOOL, CODE_SOFTWARE_QUERY_TOOL, CODEBASE_VIEW_TOOL, HEALTH_TOOL,
        INDEX_STATUS_TOOL, INSPECT_GRAPH_TOOL, RETRIEVE_CONTEXT_TOOL, SERVICE_STATUS_TOOL,
    },
};
use super::{
    builtin_tools::{
        health_tool, index_status_tool, inspect_graph_tool, retrieve_context_tool,
        service_status_tool,
    },
    server::McpServer,
};

#[derive(Debug, Deserialize)]
pub(in crate::interfaces::agent::mcp) struct ToolCallParams {
    pub(in crate::interfaces::agent::mcp) name: String,
    #[serde(default)]
    pub(in crate::interfaces::agent::mcp) arguments: Value,
}

pub(in crate::interfaces::agent::mcp) async fn run_cancellable_tool_call(
    server: &McpServer,
    params: ToolCallParams,
    request_id: String,
) -> ToolCallOutcome {
    let started = Instant::now();
    let operation = params.name.clone();
    let (mut cancellation, _registration) = server.cancellations.register(request_id.clone());
    let timeout = Duration::from_millis(server.agent.access_policy.max_runtime_ms);
    let tool = run_tool_call(server, params, request_id.clone());

    let result = tokio::select! {
        result = tokio::time::timeout(timeout, tool) => match result {
            Ok(value) => value,
            Err(_) => {
                server.qos.record_timed_out();
                tool_error_result(AgentAdapterError::new(
                    AgentAdapterErrorKind::Timeout,
                    "MCP tool call exceeded max_runtime_ms",
                ))
            }
        },
        _ = wait_for_cancellation(&mut cancellation) => {
            tool_error_result(AgentAdapterError::new(
                AgentAdapterErrorKind::Cancelled,
                "MCP tool call was cancelled",
            ))
        }
    };

    ToolCallOutcome {
        operation,
        request_id,
        result,
        duration_ms: elapsed_millis(started),
    }
}

pub(in crate::interfaces::agent::mcp) struct ToolCallOutcome {
    pub(in crate::interfaces::agent::mcp) operation: String,
    pub(in crate::interfaces::agent::mcp) request_id: String,
    pub(in crate::interfaces::agent::mcp) result: Value,
    pub(in crate::interfaces::agent::mcp) duration_ms: u64,
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    while cancellation.changed().await.is_ok() {
        if *cancellation.borrow() {
            return;
        }
    }

    std::future::pending::<()>().await;
}

async fn run_tool_call(server: &McpServer, params: ToolCallParams, request_id: String) -> Value {
    match params.name.as_str() {
        RETRIEVE_CONTEXT_TOOL => retrieve_context_tool(server, params.arguments, request_id).await,
        INSPECT_GRAPH_TOOL => inspect_graph_tool(server, params.arguments, request_id).await,
        HEALTH_TOOL => health_tool(server, request_id).await,
        SERVICE_STATUS_TOOL => service_status_tool(server, request_id).await,
        INDEX_STATUS_TOOL => index_status_tool(server, request_id).await,
        CODE_QUERY_TOOL
        | CODE_CONTEXT_TOOL
        | CODE_FEATURE_FLAGS_TOOL
        | CODE_IMPACT_TOOL
        | CODE_REPOSITORY_SET_QUERY_TOOL
        | CODE_SOFTWARE_QUERY_TOOL
        | CODEBASE_VIEW_TOOL => {
            run_code_tool(server, params.name.as_str(), params.arguments, request_id).await
        }
        _ => json!({
            "content": [{"type": "text", "text": "unknown MCP tool"}],
            "isError": true
        }),
    }
}

pub(in crate::interfaces::agent::mcp) fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "tool_runtime_tests.rs"]
mod tests;
