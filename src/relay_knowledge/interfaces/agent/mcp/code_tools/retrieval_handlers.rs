//! MCP code-retrieval tool workflows.

use serde_json::{Value, json};

use crate::{
    domain::{
        CodeGraphContextRequest, CodeRepositorySelector, CodeRepositorySetQueryRequest,
        CodeRetrievalRequest, REPOSITORY_GRAPH_DEFAULT_EDGE_LIMIT,
        REPOSITORY_GRAPH_DEFAULT_NODE_LIMIT, RepositoryGraphNeighborhoodRequest,
    },
    interfaces::agent::{
        AgentAdapterError, AgentAdapterErrorKind, authorize_limit, validate_path_texts,
        validate_query_text,
    },
};

use super::super::{
    McpServer,
    tool_contract::{
        api_error_result, domain_argument_error, invalid_arguments, parse_freshness,
        request_context, tool_error_result, tool_success_result,
    },
};
use super::{
    agent_budget::{apply_agent_code_budget, explore_budget},
    request_contracts::{
        CodeContextArgs, CodeQueryArgs, CodeRepositorySetQueryArgs, RepositoryGraphArgs,
        authorize_code_context_bytes, authorize_code_context_limit, parse_code_query_kind,
    },
};

pub(super) async fn repository_graph_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<RepositoryGraphArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    let mut validated_paths = args.path_filters.clone();
    validated_paths.push(args.focus_path.clone());
    if let Err(error) = validate_path_texts("paths", &validated_paths) {
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
                "repository is required for relay_repository_graph",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let selector = match CodeRepositorySelector::new(
        repository,
        args.ref_selector.unwrap_or_else(|| "HEAD".to_owned()),
        args.path_filters,
        vec!["markdown".to_owned()],
    ) {
        Ok(selector) => selector,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };
    let request = match RepositoryGraphNeighborhoodRequest::new(
        selector,
        args.focus_path,
        args.depth.unwrap_or(1),
        args.node_limit
            .unwrap_or(REPOSITORY_GRAPH_DEFAULT_NODE_LIMIT),
        args.edge_limit
            .unwrap_or(REPOSITORY_GRAPH_DEFAULT_EDGE_LIMIT),
    ) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };

    match server
        .service
        .repository_graph_neighborhood(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!(
                "repository graph returned {} node(s) and {} edge(s)",
                response.nodes.len(),
                response.edges.len()
            ),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn code_context_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeContextArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_query_text("query", &args.query)
        .and_then(|_| validate_path_texts("path_filters", &args.path_filters))
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
                "repository is required for relay_codegraph_context",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let limit = match authorize_code_context_limit(args.limit, &server.agent.access_policy) {
        Ok(limit) => limit,
        Err(error) => return tool_error_result(error),
    };
    let max_context_bytes = match authorize_code_context_bytes(
        args.max_context_bytes,
        server.agent.access_policy.max_context_bytes,
    ) {
        Ok(max_context_bytes) => max_context_bytes,
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
    let request = match CodeGraphContextRequest::new(
        selector,
        args.query,
        limit,
        freshness,
        max_context_bytes,
        args.include_code.unwrap_or(true),
        args.exclude_generated.unwrap_or(false),
    ) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };

    match server
        .service
        .codegraph_context(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!(
                "codegraph context returned {} entry point(s)",
                response.pack.entry_points.len()
            ),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn code_repository_set_query_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeRepositorySetQueryArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_query_text("query", &args.query)
        .and_then(|_| validate_path_texts("path_filters", &args.path_filters))
    {
        return tool_error_result(error);
    }
    let repository_set = match server
        .scope_authorizer
        .authorize_repository_set_scope(
            &server.service,
            &server.agent.access_policy,
            Some(args.repository_set),
        )
        .await
    {
        Ok(Some(repository_set)) => repository_set,
        Ok(None) => {
            return tool_error_result(AgentAdapterError::new(
                AgentAdapterErrorKind::InvalidScope,
                "repository_set is required for relay_code_repository_set_query",
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
    let mut request = match CodeRepositorySetQueryRequest::new(
        repository_set,
        args.query,
        kind,
        limit,
        freshness,
        args.path_filters,
        args.language_filters,
    ) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };
    request.exclude_generated = args.exclude_generated.unwrap_or(false);

    match server
        .service
        .query_code_repository_set(request, request_context(request_id))
        .await
    {
        Ok(response) => {
            let file_count = response
                .status
                .members
                .iter()
                .map(|member| member.indexed_file_count)
                .sum();
            let mut structured = json!(response);
            apply_agent_code_budget(
                &mut structured,
                explore_budget(file_count),
                args.include_code.unwrap_or(false),
            );
            let result_count = structured["results"].as_array().map_or(0, Vec::len);
            tool_success_result(
                format!("repository set query returned {result_count} result(s)"),
                structured,
            )
        }
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn code_query_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeQueryArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_query_text("query", &args.query)
        .and_then(|_| validate_path_texts("path_filters", &args.path_filters))
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
                "repository is required for relay_code_query",
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
    let mut request = match CodeRetrievalRequest::new(args.query, selector, kind, limit, freshness)
    {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };
    request.exclude_generated = args.exclude_generated.unwrap_or(false);

    match server
        .service
        .query_code_repository(request, request_context(request_id))
        .await
    {
        Ok(response) => {
            let budget = explore_budget(response.scope.indexed_file_count);
            let mut structured = json!(response);
            apply_agent_code_budget(&mut structured, budget, args.include_code.unwrap_or(false));
            let result_count = structured["results"].as_array().map_or(0, Vec::len);
            tool_success_result(
                format!("code query returned {result_count} result(s)"),
                structured,
            )
        }
        Err(error) => api_error_result(error),
    }
}
