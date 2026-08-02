use std::time::Instant;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{
        AgentRetrievalResult, GraphInspectionRequest, HybridRetrievalRequest, RuntimeIdentity,
        freshness_label,
    },
    interfaces::agent::{authorize_limit, validate_query_text},
};

use super::super::tool_contract::{
    api_error_result, invalid_arguments, parse_freshness, request_context, tool_error_result,
    tool_success_result,
};
use super::{server::McpServer, tool_runtime::elapsed_millis};

#[derive(Debug, Deserialize)]
struct RetrieveContextArgs {
    query: String,
    #[serde(default)]
    source_scope: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    freshness: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InspectGraphArgs {
    #[serde(default)]
    source_scope: Option<String>,
}

pub(super) async fn retrieve_context_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let started = Instant::now();
    let args = match serde_json::from_value::<RetrieveContextArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_query_text("query", &args.query) {
        return tool_error_result(error);
    }
    let policy = &server.agent.access_policy;
    let limit = match authorize_limit(args.limit, policy) {
        Ok(limit) => limit,
        Err(error) => return tool_error_result(error),
    };
    let source_scope = match server
        .scope_authorizer
        .authorize_scope(&server.service, policy, args.source_scope)
        .await
    {
        Ok(scope) => scope,
        Err(error) => return tool_error_result(error),
    };
    let freshness = match parse_freshness(args.freshness.as_deref()) {
        Ok(freshness) => freshness,
        Err(error) => return tool_error_result(error),
    };
    let context = request_context(request_id.clone());
    let identity = RuntimeIdentity::mcp(Some(request_id));

    match server
        .service
        .retrieve_context(
            HybridRetrievalRequest {
                query: args.query,
                source_scope: source_scope.clone(),
                limit,
                freshness,
            },
            context,
        )
        .await
    {
        Ok(response) => {
            let elapsed_ms = elapsed_millis(started);
            let result = AgentRetrievalResult::from_retrieval(
                response,
                identity,
                policy.max_context_bytes,
                elapsed_ms,
            );
            tool_success_result(
                format!(
                    "retrieved {} result(s), graph_version={}, freshness={}",
                    result.results.len(),
                    result.metadata.graph_version,
                    freshness_label(freshness)
                ),
                json!(result),
            )
        }
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn inspect_graph_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<InspectGraphArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    let source_scope = match server
        .scope_authorizer
        .authorize_scope(
            &server.service,
            &server.agent.access_policy,
            args.source_scope,
        )
        .await
    {
        Ok(scope) => scope,
        Err(error) => return tool_error_result(error),
    };

    match server
        .service
        .inspect_graph(
            GraphInspectionRequest { source_scope },
            request_context(request_id),
        )
        .await
    {
        Ok(response) => tool_success_result("graph inspection completed", json!(response)),
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn health_tool(server: &McpServer, request_id: String) -> Value {
    match server
        .service
        .read_only_health(request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!(
                "health={}",
                if response.healthy { "ok" } else { "degraded" }
            ),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn service_status_tool(server: &McpServer, request_id: String) -> Value {
    match server
        .service
        .read_only_service_status(request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result("service status loaded", json!(response)),
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn index_status_tool(server: &McpServer, request_id: String) -> Value {
    match server.service.health(request_context(request_id)).await {
        Ok(response) => tool_success_result(
            "index status loaded",
            json!({
                "metadata": response.metadata,
                "indexes": response.indexes,
            }),
        ),
        Err(error) => api_error_result(error),
    }
}

#[cfg(test)]
#[path = "builtin_tools_tests.rs"]
mod tests;
