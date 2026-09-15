//! Authorized MCP access to immutable per-file diagnostic pages.
use super::super::{
    McpServer,
    tool_contract::{
        api_error_result, domain_argument_error, invalid_arguments, request_context,
        tool_error_result, tool_success_result,
    },
};
use crate::{
    domain::{CodeDiagnosticsRequest, CodeRepositorySelector},
    interfaces::agent::{authorize_limit, validate_path_texts},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Args {
    repository: String,
    ref_selector: Option<String>,
    #[serde(default)]
    path_filters: Vec<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

pub(super) async fn run(server: &McpServer, arguments: Value, request_id: String) -> Value {
    let args: Args = match serde_json::from_value(arguments) {
        Ok(args) => args,
        Err(e) => return tool_error_result(invalid_arguments(e)),
    };
    if let Err(e) = validate_path_texts("path_filters", &args.path_filters) {
        return tool_error_result(e);
    }
    let repository = match server
        .scope_authorizer
        .authorize_scope(
            &server.service,
            &server.agent.access_policy,
            Some(args.repository),
        )
        .await
    {
        Ok(Some(repository)) => repository,
        Ok(None) => {
            return api_error_result(crate::api::ApiError::invalid_argument(
                "repository is required",
            ));
        }
        Err(e) => return tool_error_result(e),
    };
    let limit = match authorize_limit(
        args.limit
            .or(Some(50.min(server.agent.access_policy.max_limit))),
        &server.agent.access_policy,
    ) {
        Ok(limit) => limit,
        Err(e) => return tool_error_result(e),
    };
    let repository = match CodeRepositorySelector::new(
        repository,
        args.ref_selector.unwrap_or_else(|| "HEAD".into()),
        args.path_filters,
        Vec::new(),
    ) {
        Ok(repository) => repository,
        Err(e) => return tool_error_result(domain_argument_error(e)),
    };
    match server
        .service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository,
                limit,
                cursor: args.cursor,
            },
            request_context(request_id),
        )
        .await
    {
        Ok(response) => {
            let value = json!(response);
            if value.to_string().len() > server.agent.access_policy.max_context_bytes {
                return api_error_result(crate::api::ApiError::invalid_argument(
                    "diagnostic page exceeds agent context budget; request a smaller limit",
                ));
            }
            tool_success_result(
                format!("{} file diagnostic(s)", response.diagnostics.len()),
                value,
            )
        }
        Err(e) => api_error_result(e),
    }
}

pub(in crate::interfaces::agent::mcp) fn definition() -> Value {
    json!({"name":super::super::tool_registry::CODE_DIAGNOSTICS_TOOL,"description":"Page through per-file indexing diagnostics for an authorized immutable repository snapshot. Freshness and content completeness are independent.","inputSchema":{"type":"object","properties":{"repository":{"type":"string","minLength":1},"ref_selector":{"type":"string"},"path_filters":{"type":"array","items":{"type":"string"}},"limit":{"type":"integer","minimum":1,"maximum":200,"default":50},"cursor":{"type":"string","maxLength":16384}},"required":["repository"]}})
}
