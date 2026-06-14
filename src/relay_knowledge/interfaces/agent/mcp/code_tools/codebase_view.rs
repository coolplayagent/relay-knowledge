use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    domain::{CodeRepositorySelector, CodebaseViewKind, CodebaseViewRequest},
    interfaces::agent::{MAX_AGENT_PATH_CHARS, validate_path_texts},
};

use super::super::{
    AgentAdapterError, AgentAdapterErrorKind, McpServer, api_error_result, authorize_limit,
    domain_argument_error, invalid_arguments, parse_freshness, request_context, tool_error_result,
    tool_registry::CODEBASE_VIEW_TOOL, tool_success_result,
};

#[derive(Debug, Deserialize)]
struct CodebaseViewArgs {
    repository: String,
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
    #[serde(default)]
    changed_paths: Vec<String>,
}

pub(super) async fn run(server: &McpServer, arguments: Value, request_id: String) -> Value {
    let args = match serde_json::from_value::<CodebaseViewArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_path_texts("path_filters", &args.path_filters)
        .and_then(|_| validate_path_texts("changed_paths", &args.changed_paths))
    {
        return tool_error_result(error);
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
            return tool_error_result(AgentAdapterError::new(
                AgentAdapterErrorKind::InvalidScope,
                "repository is required for relay_codebase_view",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let requested_limit = args.limit;
    let limit = match authorize_limit(requested_limit, &server.agent.access_policy) {
        Ok(limit) => limit,
        Err(error) => return tool_error_result(error),
    };
    let limit = if requested_limit.is_none() {
        limit.min(100)
    } else {
        limit
    };
    let kind = match parse_codebase_view_kind(args.kind.as_deref().unwrap_or("architecture_layers"))
    {
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
    let request =
        match CodebaseViewRequest::new(selector, kind, freshness, limit, args.changed_paths) {
            Ok(request) => request,
            Err(error) => return tool_error_result(domain_argument_error(error)),
        };

    match server
        .service
        .codebase_view(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!(
                "codebase view returned {} section(s)",
                response.sections.len()
            ),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

pub(crate) fn definition() -> Value {
    json!({
        "name": CODEBASE_VIEW_TOOL,
        "description": "Return a deterministic, evidence-backed codebase understanding view derived from indexed graph facts.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "kind": {
                    "type": "string",
                    "enum": ["architecture_layers", "architecture-layers", "business_domains", "business-domains", "dependency_tour", "dependency-tour", "process_flow", "process-flow", "affected_scope", "affected-scope"]
                },
                "limit": {"type": "integer", "minimum": 1, "maximum": 100},
                "ref_selector": {"type": "string"},
                "path_filters": {"type": "array", "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "changed_paths": {"type": "array", "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository"]
        }
    })
}

fn parse_codebase_view_kind(value: &str) -> Result<CodebaseViewKind, AgentAdapterError> {
    match value {
        "architecture_layers" | "architecture-layers" => Ok(CodebaseViewKind::ArchitectureLayers),
        "business_domains" | "business-domains" => Ok(CodebaseViewKind::BusinessDomains),
        "dependency_tour" | "dependency-tour" => Ok(CodebaseViewKind::DependencyTour),
        "process_flow" | "process-flow" => Ok(CodebaseViewKind::ProcessFlow),
        "affected_scope" | "affected-scope" => Ok(CodebaseViewKind::AffectedScope),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid codebase view kind '{other}'"),
        )),
    }
}
