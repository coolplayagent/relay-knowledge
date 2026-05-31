use serde::Deserialize;
use serde_json::{Value, json};

use crate::domain::{
    CodeFeatureFlagRequest, CodeImpactRequest, CodeQueryKind, CodeRepositorySelector,
    CodeRepositorySetQueryRequest, CodeRetrievalRequest, SoftwareGlobalKind, SoftwareGlobalRequest,
};

use super::{
    AgentAdapterError, AgentAdapterErrorKind, McpServer, api_error_result, authorize_limit,
    domain_argument_error, invalid_arguments, parse_freshness, request_context, tool_error_result,
    tool_registry::{
        CODE_FEATURE_FLAGS_TOOL, CODE_IMPACT_TOOL, CODE_QUERY_TOOL, CODE_REPOSITORY_SET_QUERY_TOOL,
        CODE_SOFTWARE_QUERY_TOOL,
    },
    tool_success_result,
};

const CODE_QUERY_KIND_SCHEMA_VALUES: &[&str] = &[
    "hybrid",
    "symbol",
    "symbols",
    "definition",
    "definitions",
    "reference",
    "references",
    "caller",
    "callers",
    "callee",
    "callees",
    "import",
    "imports",
    "sbom",
];

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

#[derive(Debug, Deserialize)]
struct CodeFeatureFlagsArgs {
    repository: String,
    #[serde(default)]
    query: Option<String>,
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
struct CodeSoftwareQueryArgs {
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
}

#[derive(Debug, Deserialize)]
struct CodeRepositorySetQueryArgs {
    repository_set: String,
    query: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    path_filters: Vec<String>,
    #[serde(default)]
    language_filters: Vec<String>,
    #[serde(default)]
    freshness: Option<String>,
}

pub(super) async fn run_code_tool(
    server: &McpServer,
    name: &str,
    arguments: Value,
    request_id: String,
) -> Value {
    match name {
        CODE_QUERY_TOOL => code_query_tool(server, arguments, request_id).await,
        CODE_FEATURE_FLAGS_TOOL => code_feature_flags_tool(server, arguments, request_id).await,
        CODE_SOFTWARE_QUERY_TOOL => code_software_query_tool(server, arguments, request_id).await,
        CODE_IMPACT_TOOL => code_impact_tool(server, arguments, request_id).await,
        CODE_REPOSITORY_SET_QUERY_TOOL => {
            code_repository_set_query_tool(server, arguments, request_id).await
        }
        _ => tool_error_result(AgentAdapterError::new(
            AgentAdapterErrorKind::UnsupportedOperation,
            "unknown code tool",
        )),
    }
}

async fn code_software_query_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeSoftwareQueryArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
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

async fn code_repository_set_query_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeRepositorySetQueryArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
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
    let request = match CodeRepositorySetQueryRequest::new(
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

    match server
        .service
        .query_code_repository_set(request, request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result(
            format!(
                "repository set query returned {} result(s)",
                response.results.len()
            ),
            json!(response),
        ),
        Err(error) => api_error_result(error),
    }
}

async fn code_query_tool(server: &McpServer, arguments: Value, request_id: String) -> Value {
    let args = match serde_json::from_value::<CodeQueryArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
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

async fn code_feature_flags_tool(
    server: &McpServer,
    arguments: Value,
    request_id: String,
) -> Value {
    let args = match serde_json::from_value::<CodeFeatureFlagsArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
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

async fn code_impact_tool(server: &McpServer, arguments: Value, request_id: String) -> Value {
    let args = match serde_json::from_value::<CodeImpactArgs>(arguments) {
        Ok(args) => args,
        Err(error) => return tool_error_result(invalid_arguments(error)),
    };
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

pub(super) fn code_query_tool_definition() -> Value {
    json!({
        "name": CODE_QUERY_TOOL,
        "description": "Query an authorized indexed code graph repository. Unresolved external imports may include bounded current-repository grep text_fallback evidence and a diagnostic.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "query": {"type": "string", "minLength": 1},
                "kind": {
                    "type": "string",
                    "enum": CODE_QUERY_KIND_SCHEMA_VALUES
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

pub(super) fn code_feature_flags_tool_definition() -> Value {
    json!({
        "name": CODE_FEATURE_FLAGS_TOOL,
        "description": "List configuration-driven feature flags and guarded-code relationships from an authorized indexed code repository.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "query": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1},
                "ref_selector": {"type": "string"},
                "path_filters": {"type": "array", "items": {"type": "string"}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository"]
        }
    })
}

pub(super) fn code_software_query_tool_definition() -> Value {
    json!({
        "name": CODE_SOFTWARE_QUERY_TOOL,
        "description": "Read the authorized repository software global-model projection using existing kind values. Use relay_code_feature_flags for configuration-driven flag relationships.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "kind": {
                    "type": "string",
                    "enum": ["dependency", "dependencies", "sdk", "sdks", "file", "files", "topic", "topics", "relationship", "relationships", "config", "configuration", "configurations", "build", "iac", "design", "model", "models", "all"]
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
            "required": ["repository"]
        }
    })
}

pub(super) fn code_impact_tool_definition() -> Value {
    json!({
        "name": CODE_IMPACT_TOOL,
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

pub(super) fn code_repository_set_query_tool_definition() -> Value {
    json!({
        "name": CODE_REPOSITORY_SET_QUERY_TOOL,
        "description": "Query an authorized repository set across multiple indexed code graph snapshots. Unresolved external imports may include bounded current-repository grep text_fallback evidence and a diagnostic.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository_set": {"type": "string", "minLength": 1},
                "query": {"type": "string", "minLength": 1},
                "kind": {
                    "type": "string",
                    "enum": CODE_QUERY_KIND_SCHEMA_VALUES
                },
                "limit": {"type": "integer", "minimum": 1},
                "path_filters": {"type": "array", "items": {"type": "string"}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository_set", "query"]
        }
    })
}

fn parse_code_query_kind(value: &str) -> Result<CodeQueryKind, AgentAdapterError> {
    match value {
        "hybrid" => Ok(CodeQueryKind::Hybrid),
        "symbol" | "symbols" => Ok(CodeQueryKind::Symbol),
        "definition" | "definitions" => Ok(CodeQueryKind::Definition),
        "reference" | "references" => Ok(CodeQueryKind::References),
        "caller" | "callers" => Ok(CodeQueryKind::Callers),
        "callee" | "callees" => Ok(CodeQueryKind::Callees),
        "import" | "imports" => Ok(CodeQueryKind::Imports),
        "sbom" => Ok(CodeQueryKind::Sbom),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid code query kind '{other}'"),
        )),
    }
}

fn parse_software_query_kind(value: &str) -> Result<SoftwareGlobalKind, AgentAdapterError> {
    match value {
        "dependency" | "dependencies" => Ok(SoftwareGlobalKind::Dependencies),
        "sdk" | "sdks" => Ok(SoftwareGlobalKind::Sdks),
        "file" | "files" => Ok(SoftwareGlobalKind::Files),
        "topic" | "topics" => Ok(SoftwareGlobalKind::Topics),
        "relationship" | "relationships" | "config" | "configuration" | "configurations" => {
            Ok(SoftwareGlobalKind::Relationships)
        }
        "build" => Ok(SoftwareGlobalKind::Build),
        "iac" => Ok(SoftwareGlobalKind::Iac),
        "design" | "model" | "models" => Ok(SoftwareGlobalKind::Design),
        "all" => Ok(SoftwareGlobalKind::All),
        other => Err(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidArgument,
            format!("invalid software query kind '{other}'"),
        )),
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

#[cfg(test)]
mod tests {
    use super::{
        code_query_tool_definition, code_repository_set_query_tool_definition,
        code_software_query_tool_definition, parse_code_query_kind, parse_software_query_kind,
    };
    use crate::domain::{CodeQueryKind, SoftwareGlobalKind};

    #[test]
    fn agent_kind_aliases_normalize_to_existing_code_and_software_kinds() {
        assert_eq!(
            parse_code_query_kind("caller").unwrap(),
            CodeQueryKind::Callers
        );
        assert_eq!(
            parse_software_query_kind("dependency").unwrap(),
            SoftwareGlobalKind::Dependencies
        );
        assert_eq!(
            parse_software_query_kind("configuration").unwrap(),
            SoftwareGlobalKind::Relationships
        );
        assert_eq!(
            parse_software_query_kind("models").unwrap(),
            SoftwareGlobalKind::Design
        );
    }

    #[test]
    fn code_tool_schemas_advertise_agent_aliases() {
        for definition in [
            code_query_tool_definition(),
            code_repository_set_query_tool_definition(),
        ] {
            let values = definition["inputSchema"]["properties"]["kind"]["enum"]
                .as_array()
                .expect("kind enum should be an array");

            for alias in [
                "symbols",
                "definitions",
                "reference",
                "caller",
                "callee",
                "import",
            ] {
                assert!(
                    values.iter().any(|value| value == alias),
                    "schema should advertise {alias}"
                );
            }
        }
    }

    #[test]
    fn software_tool_schema_advertises_agent_aliases() {
        let definition = code_software_query_tool_definition();
        let values = definition["inputSchema"]["properties"]["kind"]["enum"]
            .as_array()
            .expect("kind enum should be an array");

        for alias in ["dependency", "configuration", "models"] {
            assert!(
                values.iter().any(|value| value == alias),
                "schema should advertise {alias}"
            );
        }
    }
}
