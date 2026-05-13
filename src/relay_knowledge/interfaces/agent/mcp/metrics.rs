use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::time::Duration;

use super::{
    McpMethodError, McpServer, admit_mcp_request, endpoint_child, http_contract::validate_origin,
    request_context,
};

pub(super) fn metrics_endpoint(endpoint: &str) -> String {
    endpoint_child(endpoint, "metrics")
}

pub(super) async fn handle_metrics_get(
    State(server): State<McpServer>,
    headers: HeaderMap,
) -> Response {
    if let Err(status) = validate_origin(&server, &headers) {
        return status.into_response();
    }
    let permit = match admit_mcp_request(&server) {
        Ok(permit) => permit,
        Err(_) => return StatusCode::TOO_MANY_REQUESTS.into_response(),
    };
    let timeout = Duration::from_millis(server.agent.access_policy.max_runtime_ms);
    let result = tokio::time::timeout(timeout, prometheus_metrics(&server, "metrics-get")).await;
    drop(permit);

    match result {
        Ok(Ok(metrics)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            metrics,
        )
            .into_response(),
        Ok(Err(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain")],
            error.message,
        )
            .into_response(),
        Err(_) => (
            StatusCode::REQUEST_TIMEOUT,
            [(header::CONTENT_TYPE, "text/plain")],
            "metrics endpoint exceeded max_runtime_ms".to_owned(),
        )
            .into_response(),
    }
}

pub(super) async fn prometheus_metrics(
    server: &McpServer,
    request_id: &str,
) -> Result<String, McpMethodError> {
    let health = server
        .service
        .health(request_context(request_id.to_owned()))
        .await
        .map_err(McpMethodError::api)?;
    let qos = server.qos.snapshot();
    let mut output = String::new();
    push_metric(
        &mut output,
        "relay_knowledge_graph_version",
        "Current committed graph version.",
        health.graph.graph_version.get(),
    );
    push_metric(
        &mut output,
        "relay_knowledge_index_refresh_queue_depth",
        "Pending index refresh task count.",
        health.index_refresh.queue_depth,
    );
    push_metric(
        &mut output,
        "relay_knowledge_index_refresh_dead_letter_count",
        "Dead-lettered index refresh task count.",
        health.index_refresh.dead_letter_count,
    );
    push_metric(
        &mut output,
        "relay_knowledge_qos_in_flight_requests",
        "Current admitted MCP request count.",
        qos.in_flight_requests,
    );
    push_metric(
        &mut output,
        "relay_knowledge_qos_queued_requests",
        "Current queued MCP request count.",
        qos.queued_requests,
    );
    for index in &health.indexes {
        output.push_str(&format!(
            "relay_knowledge_index_stale{{kind=\"{}\"}} {}\n",
            index.kind.as_str(),
            usize::from(index.is_stale_for(health.graph.graph_version))
        ));
    }

    Ok(output)
}

fn push_metric(
    output: &mut String,
    name: &'static str,
    description: &'static str,
    value: impl ToString,
) {
    output.push_str(&format!("# HELP {name} {description}\n"));
    output.push_str(&format!("# TYPE {name} gauge\n"));
    output.push_str(&format!("{name} {}\n", value.to_string()));
}
