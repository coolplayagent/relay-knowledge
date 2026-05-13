use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    McpMethodError, McpServer,
    audit_bridge::{McpMethodAudit, record_mcp_method_audit},
    elapsed_millis, request_context,
};
use crate::interfaces::agent::{AgentAdapterError, AgentAdapterErrorKind, AgentAuditStatus};

const SERVICE_STATUS_URI: &str = "relay://service/status";
const HEALTH_URI: &str = "relay://service/health";
const INDEX_STATUS_URI: &str = "relay://indexes/status";
const GRAPH_SUMMARY_URI: &str = "relay://graph/summary";
const METRICS_URI: &str = "relay://metrics/prometheus";

#[derive(Debug, Deserialize)]
struct ResourceReadParams {
    uri: String,
    #[serde(default)]
    source_scope: Option<String>,
}

pub(super) fn list_resources(server: &McpServer) -> Value {
    let graph_summary = server.agent.access_policy.allow_unspecified_scope.then(|| {
        resource_descriptor(
            GRAPH_SUMMARY_URI,
            "relay.graph_summary",
            "Graph aggregate counts",
            "application/json",
        )
    });
    let mut resources = vec![
        resource_descriptor(
            SERVICE_STATUS_URI,
            "relay.service_status",
            "Resident service status",
            "application/json",
        ),
        resource_descriptor(
            HEALTH_URI,
            "relay.health",
            "Graph and index health",
            "application/json",
        ),
        resource_descriptor(
            INDEX_STATUS_URI,
            "relay.index_status",
            "Derived retrieval index status",
            "application/json",
        ),
        resource_descriptor(
            METRICS_URI,
            "relay.metrics",
            "Prometheus text metrics",
            "text/plain",
        ),
    ];
    if let Some(graph_summary) = graph_summary {
        resources.push(graph_summary);
    }

    json!({ "resources": resources })
}

pub(super) async fn read_resource_with_timeout(
    server: &McpServer,
    params: Value,
    request_id: &str,
) -> Result<Value, McpMethodError> {
    let started = Instant::now();
    let timeout = Duration::from_millis(server.agent.access_policy.max_runtime_ms);
    match tokio::time::timeout(timeout, read_resource(server, params, request_id)).await {
        Ok(result) => result,
        Err(_) => {
            record_mcp_method_audit(
                server,
                McpMethodAudit {
                    operation: "resources/read",
                    request_id,
                    status: AgentAuditStatus::Failed,
                    source_scope: None,
                    result_count: None,
                    elapsed_ms: elapsed_millis(started),
                    error_kind: Some("timeout"),
                },
            );
            Err(McpMethodError::timeout(
                "resources/read exceeded max_runtime_ms",
            ))
        }
    }
}

async fn read_resource(
    server: &McpServer,
    params: Value,
    request_id: &str,
) -> Result<Value, McpMethodError> {
    let started = Instant::now();
    let params = serde_json::from_value::<ResourceReadParams>(params).map_err(|error| {
        McpMethodError::invalid_params(format!("invalid resources/read params: {error}"))
    })?;
    let result = match params.uri.as_str() {
        SERVICE_STATUS_URI => service_status_content(server, request_id).await,
        HEALTH_URI => health_content(server, request_id).await,
        INDEX_STATUS_URI => index_status_content(server, request_id).await,
        GRAPH_SUMMARY_URI => {
            authorize_graph_summary(server, params.source_scope)?;
            graph_summary_content(server, request_id).await
        }
        METRICS_URI => metrics_content(server, request_id).await,
        _ => Err(McpMethodError::invalid_params("unknown resource uri")),
    };
    record_mcp_method_audit(
        server,
        McpMethodAudit {
            operation: "resources/read",
            request_id,
            status: if result.is_ok() {
                AgentAuditStatus::Completed
            } else {
                AgentAuditStatus::Failed
            },
            source_scope: None,
            result_count: None,
            elapsed_ms: elapsed_millis(started),
            error_kind: result.as_ref().err().map(|error| error.kind),
        },
    );

    result
}

fn authorize_graph_summary(
    server: &McpServer,
    source_scope: Option<String>,
) -> Result<(), McpMethodError> {
    if !server.agent.access_policy.allow_unspecified_scope {
        return Err(McpMethodError::adapter(AgentAdapterError::new(
            AgentAdapterErrorKind::InvalidScope,
            "relay://graph/summary requires MCP allow_unspecified_scope=true",
        )));
    }
    if source_scope.is_some() {
        return Err(McpMethodError::invalid_params(
            "relay://graph/summary is graph-wide and does not accept source_scope",
        ));
    }

    Ok(())
}

async fn service_status_content(
    server: &McpServer,
    request_id: &str,
) -> Result<Value, McpMethodError> {
    let response = server
        .service
        .service_status(request_context(request_id.to_owned()))
        .await
        .map_err(McpMethodError::api)?;
    json_content(SERVICE_STATUS_URI, &response)
}

async fn health_content(server: &McpServer, request_id: &str) -> Result<Value, McpMethodError> {
    let response = server
        .service
        .health(request_context(request_id.to_owned()))
        .await
        .map_err(McpMethodError::api)?;
    json_content(HEALTH_URI, &response)
}

async fn index_status_content(
    server: &McpServer,
    request_id: &str,
) -> Result<Value, McpMethodError> {
    let response = server
        .service
        .health(request_context(request_id.to_owned()))
        .await
        .map_err(McpMethodError::api)?;
    json_content(
        INDEX_STATUS_URI,
        &json!({
            "metadata": response.metadata,
            "indexes": response.indexes,
            "index_cursors": response.index_cursors,
            "index_refresh": response.index_refresh
        }),
    )
}

async fn graph_summary_content(
    server: &McpServer,
    request_id: &str,
) -> Result<Value, McpMethodError> {
    let response = server
        .service
        .inspect_graph(
            crate::api::GraphInspectionRequest { source_scope: None },
            request_context(request_id.to_owned()),
        )
        .await
        .map_err(McpMethodError::api)?;
    json_content(GRAPH_SUMMARY_URI, &response)
}

async fn metrics_content(server: &McpServer, request_id: &str) -> Result<Value, McpMethodError> {
    let text = super::metrics::prometheus_metrics(server, request_id).await?;
    Ok(text_content(METRICS_URI, "text/plain", text))
}

fn resource_descriptor(uri: &str, name: &str, description: &str, mime_type: &str) -> Value {
    json!({
        "uri": uri,
        "name": name,
        "description": description,
        "mimeType": mime_type
    })
}

fn json_content<T: serde::Serialize>(uri: &str, value: &T) -> Result<Value, McpMethodError> {
    let text = serde_json::to_string(value)
        .map_err(|error| McpMethodError::internal(format!("failed to encode resource: {error}")))?;
    Ok(text_content(uri, "application/json", text))
}

fn text_content(uri: &str, mime_type: &str, text: String) -> Value {
    json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": mime_type,
                "text": text
            }
        ]
    })
}
