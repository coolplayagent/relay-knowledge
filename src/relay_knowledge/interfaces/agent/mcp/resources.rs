use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{AgentAccessPolicy, GraphInspectionRequest};

use super::{McpServer, request_context};

#[derive(Debug, Deserialize)]
pub(super) struct ResourceReadParams {
    pub(super) uri: String,
}

pub(super) fn resources_list_result(policy: &AgentAccessPolicy) -> Value {
    json!({
        "resources": [
            resource_definition("relay://graph/metadata", "Graph metadata", "Current graph version and aggregate counts."),
            resource_definition("relay://graph/schema", "Graph schema", "Entity, relation, retrieval index, and code graph schema summary."),
            resource_definition("relay://scopes", "Authorized scopes", "Source scopes authorized by the current access policy."),
            resource_definition("relay://indexes/status", "Index status", "Derived retrieval index freshness and cursor status."),
            resource_definition("relay://diagnostics/current", "Current diagnostics", "Health and service diagnostics with sensitive local details reduced.")
        ],
        "authorizedScopeCount": policy.allowed_scopes.len()
    })
}

pub(super) async fn read_resource(
    server: &McpServer,
    params: ResourceReadParams,
    request_id: String,
) -> Value {
    let content = match params.uri.as_str() {
        "relay://graph/metadata" => match server
            .service
            .inspect_graph(
                GraphInspectionRequest { source_scope: None },
                request_context(request_id.clone()),
            )
            .await
        {
            Ok(response) => json!({
                "metadata": response.metadata,
                "graph": response.graph,
                "repository_code_totals": response.repository_code_totals,
            }),
            Err(error) => return resource_error_content(&params.uri, error.message),
        },
        "relay://graph/schema" => graph_schema_resource(),
        "relay://scopes" => json!({
            "allowed_scopes": server.agent.access_policy.allowed_scopes,
            "allow_unspecified_scope": server.agent.access_policy.allow_unspecified_scope,
        }),
        "relay://indexes/status" => match server
            .service
            .health(request_context(request_id.clone()))
            .await
        {
            Ok(response) => json!({
                "metadata": response.metadata,
                "indexes": response.indexes,
                "index_cursors": response.index_cursors,
                "index_refresh": response.index_refresh,
            }),
            Err(error) => return resource_error_content(&params.uri, error.message),
        },
        "relay://diagnostics/current" => match server
            .service
            .service_status(request_context(request_id.clone()))
            .await
        {
            Ok(mut response) => {
                response.service_definition_path = redacted_path(&response.service_definition_path);
                json!(response)
            }
            Err(error) => return resource_error_content(&params.uri, error.message),
        },
        _ => {
            return json!({
                "contents": [],
                "isError": true,
                "error": {
                    "error_kind": "invalid_argument",
                    "message": "unknown MCP resource URI"
                }
            });
        }
    };

    json!({
        "contents": [{
            "uri": params.uri,
            "mimeType": "application/json",
            "text": content.to_string()
        }]
    })
}

fn resource_definition(uri: &str, name: &str, description: &str) -> Value {
    json!({
        "uri": uri,
        "name": name,
        "description": description,
        "mimeType": "application/json"
    })
}

fn graph_schema_resource() -> Value {
    json!({
        "entities": ["entity", "evidence", "claim", "event", "code_file", "code_symbol", "code_reference", "code_chunk"],
        "relations": ["domain relations", "code imports", "code calls", "code symbol references"],
        "indexes": ["bm25", "semantic", "vector"],
        "freshness": ["allow-stale", "wait-until-fresh", "graph-only"]
    })
}

fn resource_error_content(uri: &str, message: String) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": json!({
                "error_kind": "storage_unavailable",
                "message": message
            }).to_string()
        }],
        "isError": true
    })
}

fn redacted_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("<service-dir>/{name}"))
        .unwrap_or_else(|| "<redacted>".to_owned())
}
