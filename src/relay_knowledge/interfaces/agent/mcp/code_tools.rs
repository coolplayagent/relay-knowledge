use serde::Deserialize;
use serde_json::{Value, json};

use crate::domain::{
    CodeImpactRequest, CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest,
};

use super::{
    AgentAdapterError, AgentAdapterErrorKind, McpServer, api_error_result, authorize_limit,
    authorize_scope, domain_argument_error, invalid_arguments, parse_freshness, request_context,
    tool_error_result, tool_success_result,
};

#[derive(Debug, Deserialize)]
struct CodeQueryArgs {
    repository: String,
    query: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    ref_selector: Option<String>,
    #[serde(default)]
    path_filters: Vec<String>,
    #[serde(default)]
    language_filters: Vec<String>,
    #[serde(default)]
    freshness: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodeImpactArgs {
    repository: String,
    base_ref: String,
    head_ref: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    path_filters: Vec<String>,
    #[serde(default)]
    language_filters: Vec<String>,
}

pub(super) async fn run_code_tool(
    server: &McpServer,
    name: &str,
    arguments: Value,
    request_id: String,
) -> Value {
    match name {
        "relay.code_query" => code_query_tool(server, arguments, request_id).await,
        "relay.code_impact" => code_impact_tool(server, arguments, request_id).await,
        _ => tool_error_result(AgentAdapterError::new(
            AgentAdapterErrorKind::UnsupportedOperation,
            "unknown code tool",
        )),
    }
}

async fn code_query_tool(server: &McpServer, arguments: Value, request_id: String) -> Value {
    let args = match serde_json::from_value::<CodeQueryArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    let repository = match authorize_scope(Some(args.repository), &server.agent.access_policy) {
        Ok(Some(repository)) => repository,
        Ok(None) => {
            return tool_error_result(AgentAdapterError::new(
                AgentAdapterErrorKind::InvalidScope,
                "repository is required for relay.code_query",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let limit = match authorize_limit(args.limit, &server.agent.access_policy) {
        Ok(limit) => limit,
        Err(error) => return tool_error_result(error),
    };
    let kind = match parse_code_query_kind(args.kind.as_deref().unwrap_or("hybrid")) {
        Ok(kind) => kind,
        Err(error) => return tool_error_result(error),
    };
    let freshness = match parse_freshness(args.freshness.as_deref()) {
        Ok(freshness) => freshness,
        Err(error) => return tool_error_result(error),
    };
    let selector = match CodeRepositorySelector::new(
        repository,
        args.ref_selector.unwrap_or_else(|| "HEAD".to_owned()),
        args.path_filters,
        args.language_filters,
    ) {
        Ok(selector) => selector,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };
    let request = match CodeRetrievalRequest::new(args.query, selector, kind, limit, freshness) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };

    match server
        .service
        .query_code_repository(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!("code query returned {} result(s)", response.results.len()),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

async fn code_impact_tool(server: &McpServer, arguments: Value, request_id: String) -> Value {
    let args = match serde_json::from_value::<CodeImpactArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    let repository = match authorize_scope(Some(args.repository), &server.agent.access_policy) {
        Ok(Some(repository)) => repository,
        Ok(None) => {
            return tool_error_result(AgentAdapterError::new(
                AgentAdapterErrorKind::InvalidScope,
                "repository is required for relay.code_impact",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let limit = match authorize_limit(args.limit, &server.agent.access_policy) {
        Ok(limit) => limit,
        Err(error) => return tool_error_result(error),
    };
    let selector = match CodeRepositorySelector::new(
        repository,
        args.head_ref.clone(),
        args.path_filters,
        args.language_filters,
    ) {
        Ok(selector) => selector,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };
    let request = match CodeImpactRequest::new(selector, args.base_ref, args.head_ref, limit) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };

    match server
        .service
        .impact_code_repository(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!("code impact returned {} result(s)", response.results.len()),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

pub(super) fn code_query_tool_definition() -> Value {
    json!({
        "name": "relay.code_query",
        "description": "Query an authorized indexed code graph repository.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "query": {"type": "string", "minLength": 1},
                "kind": {
                    "type": "string",
                    "enum": ["hybrid", "symbol", "definition", "references", "callers", "callees", "imports"]
                },
                "limit": {"type": "integer", "minimum": 1},
                "ref_selector": {"type": "string"},
                "path_filters": {"type": "array", "items": {"type": "string"}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository", "query"]
        }
    })
}

pub(super) fn code_impact_tool_definition() -> Value {
    json!({
        "name": "relay.code_impact",
        "description": "Analyze impact for a Git diff against an authorized indexed code repository.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "base_ref": {"type": "string", "minLength": 1},
                "head_ref": {"type": "string", "minLength": 1},
                "limit": {"type": "integer", "minimum": 1},
                "path_filters": {"type": "array", "items": {"type": "string"}},
                "language_filters": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["repository", "base_ref", "head_ref"]
        }
    })
}

fn parse_code_query_kind(value: &str) -> Result<CodeQueryKind, AgentAdapterError> {
    match value {
        "hybrid" => Ok(CodeQueryKind::Hybrid),
        "symbol" => Ok(CodeQueryKind::Symbol),
        "definition" => Ok(CodeQueryKind::Definition),
        "references" => Ok(CodeQueryKind::References),
        "callers" => Ok(CodeQueryKind::Callers),
        "callees" => Ok(CodeQueryKind::Callees),
        "imports" => Ok(CodeQueryKind::Imports),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid code query kind '{other}'"),
        )),
    }
}
