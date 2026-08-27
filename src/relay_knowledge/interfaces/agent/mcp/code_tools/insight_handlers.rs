//! MCP software, feature-flag, and impact insight workflows.

use serde_json::{Value, json};

use crate::{
    domain::{
        BusinessKnowledgeQueryRequest, CodeFeatureFlagRequest, CodeImpactRequest,
        CodeRepositorySelector, FrameworkGraphRequest, SoftwareGlobalRequest,
    },
    interfaces::agent::{
        AgentAdapterError, AgentAdapterErrorKind, authorize_limit, validate_optional_query_text,
        validate_path_texts,
    },
};

use super::super::{
    McpServer,
    tool_contract::{
        api_error_result, domain_argument_error, invalid_arguments, parse_freshness,
        request_context, tool_error_result, tool_success_result,
    },
};
use super::request_contracts::{
    CodeBusinessQueryArgs, CodeFeatureFlagsArgs, CodeFrameworkGraphArgs, CodeImpactArgs,
    CodeSoftwareQueryArgs, parse_business_query_kind, parse_software_query_kind,
};

pub(super) async fn code_business_query_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeBusinessQueryArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_optional_query_text("query", args.query.as_deref())
        .and_then(|_| validate_optional_query_text("domain", args.domain.as_deref()))
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
                "repository is required for relay_business_query",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let limit = match authorize_limit(args.limit, &server.agent.access_policy) {
        Ok(limit) => limit,
        Err(error) => return tool_error_result(error),
    };
    let kind = match parse_business_query_kind(args.kind.as_deref().unwrap_or("all")) {
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
        Vec::new(),
        Vec::new(),
    ) {
        Ok(selector) => selector,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };
    let request = match BusinessKnowledgeQueryRequest::new(
        selector,
        args.domain,
        args.query,
        kind,
        freshness,
        limit,
    ) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };
    match server
        .service
        .business_knowledge_query(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!(
                "business knowledge query returned {} term(s)",
                response.terms.len()
            ),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn code_software_query_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeSoftwareQueryArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_path_texts("path_filters", &args.path_filters) {
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
                "repository is required for relay_software_query",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let limit = match authorize_limit(args.limit, &server.agent.access_policy) {
        Ok(limit) => limit,
        Err(error) => return tool_error_result(error),
    };
    let kind = match parse_software_query_kind(args.kind.as_deref().unwrap_or("all")) {
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
    let request = match SoftwareGlobalRequest::new(selector, kind, freshness, limit) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };

    match server
        .service
        .software_global_projection(request, request_context(request_id))
        .await
    {
        Ok(response) => {
            let count = software_projection_result_count(&response);
            tool_success_result(
                format!("software query returned {count} result(s)"),
                json!(response),
            )
        }
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn code_feature_flags_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeFeatureFlagsArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_optional_query_text("query", args.query.as_deref())
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
                "repository is required for relay_code_feature_flags",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let limit = match authorize_limit(args.limit, &server.agent.access_policy) {
        Ok(limit) => limit,
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
    let request = match CodeFeatureFlagRequest::new(args.query, selector, limit, freshness) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };

    match server
        .service
        .query_code_repository_feature_flags(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!(
                "feature flag query returned {} flag group(s)",
                response.flags.len()
            ),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn code_framework_graph_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeFrameworkGraphArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_optional_query_text("query", args.query.as_deref())
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
                "repository is required for relay_code_framework",
            ));
        }
        Err(error) => return tool_error_result(error),
    };
    let limit = match authorize_limit(args.limit, &server.agent.access_policy) {
        Ok(limit) => limit,
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
        Vec::new(),
    ) {
        Ok(selector) => selector,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };
    let request = match FrameworkGraphRequest::new(
        args.query,
        selector,
        args.frameworks,
        args.kinds,
        limit,
        freshness,
    ) {
        Ok(request) => request,
        Err(error) => return tool_error_result(domain_argument_error(error)),
    };

    match server
        .service
        .query_code_repository_framework_graph(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!(
                "framework graph returned {} node(s) and {} edge(s)",
                response.graph.nodes.len(),
                response.graph.edges.len()
            ),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

pub(super) async fn code_impact_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeImpactArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
    if let Err(error) = validate_path_texts("path_filters", &args.path_filters) {
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
                "repository is required for relay_code_impact",
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

fn software_projection_result_count(response: &crate::api::SoftwareGlobalResponse) -> usize {
    response.components.len()
        + response.dependency_usages.len()
        + response.sdk_usages.len()
        + response.files.len()
        + response.topics.len()
        + response.relationships.len()
        + response.build_targets.len()
        + response.iac_resources.len()
        + response.design_elements.len()
}
