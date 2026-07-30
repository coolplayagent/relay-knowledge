use std::{
    error::Error,
    fmt,
    future::Future,
    time::{Duration, Instant},
};

mod audit_bridge;
mod code_tools;
mod http_contract;
mod json_rpc;
mod metrics;
mod notifications;
mod prompts;
mod resources;
mod scope_authorization;
mod state;
mod tool_contract;
mod tool_registry;

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use http_contract::{
    ensure_remote_bind_allowed, validate_http_headers, validate_origin,
    validate_protocol_version_header,
};
use json_rpc::{
    initialize_result, invalid_request_id_response, is_json_rpc_id, is_valid_json_rpc_response,
    json_rpc_error, json_rpc_success, json_rpc_success_with_session, request_id_key,
    response_message_session_response, session_create_error, session_lookup_error_response,
    uninitialized_session_response, validate_initialize_params,
};
use scope_authorization::RuntimeScopeAuthorizer;
use state::{CancellationRegistry, SessionLookupError, SessionRegistry};
use tool_contract::{
    api_error_result, invalid_arguments, parse_freshness, request_context, tool_error_result,
    tool_success_result,
};

use crate::{
    api::{
        AgentRetrievalResult, ApiError, ErrorKind, GraphInspectionRequest, HybridRetrievalRequest,
        RuntimeIdentity, freshness_label,
    },
    application::{AgentRuntimeConfig, RelayKnowledgeService},
    net::{
        NetworkRuntime,
        http::HttpServeError,
        qos::{QosPermit, QosRuntime, RejectReason},
    },
    observability::AgentProtocolMetrics,
};

use super::{
    AgentAdapterError, AgentAdapterErrorKind, AgentAuditEvent, AgentAuditLog, AgentAuditSink,
    authorize_limit, validate_query_text,
};
use audit_bridge::{record_mcp_qos_rejection, record_mcp_tool_audit};
use code_tools::run_code_tool;
use tool_registry::{
    CODE_CONTEXT_TOOL, CODE_FEATURE_FLAGS_TOOL, CODE_IMPACT_TOOL, CODE_QUERY_TOOL,
    CODE_REPOSITORY_SET_QUERY_TOOL, CODE_SOFTWARE_QUERY_TOOL, CODEBASE_VIEW_TOOL, HEALTH_TOOL,
    INDEX_STATUS_TOOL, INSPECT_GRAPH_TOOL, RETRIEVE_CONTEXT_TOOL, SERVICE_STATUS_TOOL,
    is_known_tool,
};

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";

/// MCP Streamable HTTP server state shared by route handlers.
#[derive(Clone)]
pub struct McpServer {
    service: RelayKnowledgeService,
    network: NetworkRuntime,
    agent: AgentRuntimeConfig,
    qos: QosRuntime,
    audit: AgentAuditLog,
    metrics: AgentProtocolMetrics,
    cancellations: CancellationRegistry,
    sessions: SessionRegistry,
    scope_authorizer: RuntimeScopeAuthorizer,
}

impl McpServer {
    /// Creates MCP server state from validated runtime boundaries.
    pub fn new(
        service: RelayKnowledgeService,
        network: NetworkRuntime,
        agent: AgentRuntimeConfig,
    ) -> Self {
        let metrics = service.observability().agent_metrics();
        let qos = network.qos_runtime();
        let audit = if agent.audit_sink_enabled {
            AgentAuditSink::jsonl(service.agent_audit_log_path(), agent.audit_queue_depth)
                .map(AgentAuditLog::with_sink)
                .unwrap_or_default()
        } else {
            AgentAuditLog::default()
        };

        Self {
            service,
            network,
            agent,
            qos,
            audit,
            metrics,
            cancellations: CancellationRegistry::default(),
            sessions: SessionRegistry::default(),
            scope_authorizer: RuntimeScopeAuthorizer::default(),
        }
    }

    /// Builds the Streamable HTTP router without opening sockets.
    pub fn router(self) -> Router {
        let config = self.network.current();
        let endpoint = self.agent.mcp_endpoint.clone();
        let metrics_endpoint = metrics::metrics_endpoint(&endpoint);
        let body_limit = usize::try_from(config.http.max_request_body_bytes).unwrap_or(usize::MAX);

        Router::new()
            .route(&endpoint, post(handle_mcp_post))
            .route(&endpoint, axum::routing::delete(handle_mcp_delete))
            .route(&metrics_endpoint, get(metrics::handle_metrics_get))
            .with_state(self)
            .layer(TraceLayer::new_for_http())
            .layer(RequestBodyLimitLayer::new(body_limit))
    }

    /// Starts the MCP HTTP listener through `net::http`.
    pub async fn serve_until_shutdown(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), McpServeError> {
        let network_config = self.network.current();
        let config = network_config.http;
        let qos_policy = network_config.qos;
        let qos = self.qos.clone();
        let router = self.checked_router()?;

        crate::net::http::serve_router_with_qos(router, config, qos, qos_policy, shutdown)
            .await
            .map_err(McpServeError::Http)
    }

    /// Builds the Streamable HTTP router after validating listener policy.
    pub fn checked_router(self) -> Result<Router, McpServeError> {
        if !self.agent.mcp_streamable_http_enabled {
            return Err(McpServeError::Disabled);
        }
        ensure_remote_bind_allowed(&self.network.current().http, &self.agent.access_policy)?;

        Ok(self.router())
    }

    #[cfg(test)]
    pub fn qos_snapshot(&self) -> crate::net::qos::QosSnapshot {
        self.qos.snapshot()
    }

    #[cfg(test)]
    pub fn audit_snapshot(&self) -> Vec<AgentAuditEvent> {
        self.audit.snapshot()
    }
}

/// MCP server startup error.
#[derive(Debug)]
pub enum McpServeError {
    Disabled,
    RemoteBindDisabled,
    Http(HttpServeError),
}

impl fmt::Display for McpServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(formatter, "MCP Streamable HTTP is not enabled"),
            Self::RemoteBindDisabled => {
                write!(
                    formatter,
                    "MCP remote bind requires allow_remote_clients=true"
                )
            }
            Self::Http(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for McpServeError {}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: Option<String>,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    capabilities: Value,
    #[serde(rename = "clientInfo")]
    client_info: InitializeClientInfo,
}

#[derive(Debug, Deserialize)]
struct InitializeClientInfo {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

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

#[derive(Debug, Deserialize)]
pub(super) struct CancelParams {
    #[serde(rename = "requestId")]
    request_id: Value,
}

pub(super) struct McpMethodError {
    code: i64,
    kind: &'static str,
    message: String,
}

impl McpMethodError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            kind: "invalid_argument",
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            kind: "internal",
            message: message.into(),
        }
    }

    fn timeout(message: impl Into<String>) -> Self {
        Self {
            code: -32000,
            kind: "timeout",
            message: message.into(),
        }
    }

    fn api(error: ApiError) -> Self {
        Self {
            code: -32000,
            kind: match error.error_kind {
                ErrorKind::InvalidArgument => "invalid_argument",
                ErrorKind::StorageUnavailable => "storage_unavailable",
                ErrorKind::QosRejected => "qos_rejected",
                ErrorKind::Timeout => "timeout",
                ErrorKind::Internal => "internal",
            },
            message: error.message,
        }
    }

    fn adapter(error: AgentAdapterError) -> Self {
        Self {
            code: -32000,
            kind: error.kind.as_str(),
            message: error.message,
        }
    }
}

async fn handle_mcp_post(
    State(server): State<McpServer>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(status) = validate_http_headers(&server, &headers) {
        return status.into_response();
    }
    if body.len() as u64 > server.network.current().http.max_request_body_bytes {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let payload = match serde_json::from_slice::<Value>(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return json_rpc_error(Value::Null, -32700, format!("parse error: {error}"));
        }
    };
    if payload.is_array() {
        return json_rpc_error(Value::Null, -32600, "batch requests are not supported");
    }
    let request = match serde_json::from_value::<JsonRpcRequest>(payload.clone()) {
        Ok(request) => request,
        Err(error) => {
            return json_rpc_error(Value::Null, -32600, format!("invalid request: {error}"));
        }
    };
    let id = request.id.clone().unwrap_or(Value::Null);
    if request.jsonrpc.as_deref() != Some("2.0") {
        return json_rpc_error(id, -32600, "jsonrpc must be 2.0");
    }
    let Some(method) = request.method.as_deref() else {
        if is_valid_json_rpc_response(&payload) {
            if let Err(status) = validate_protocol_version_header(&headers, true) {
                return status.into_response();
            }
            return response_message_session_response(&server, &headers);
        }
        if payload
            .as_object()
            .is_some_and(|object| object.contains_key("result") || object.contains_key("error"))
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
        return json_rpc_error(id, -32600, "method is required");
    };

    if method == "initialize" {
        let Some(id) = request.id else {
            return json_rpc_error(Value::Null, -32600, "requests must include an id");
        };
        if !is_json_rpc_id(&id) {
            return invalid_request_id_response();
        }
        if let Err(message) = validate_initialize_params(request.params) {
            return json_rpc_error(id, -32602, message);
        }
        let Ok(permit) = admit_mcp_request(&server) else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        let session_id = match server.sessions.require_session(&headers) {
            Ok(session) => session.session_id().to_owned(),
            Err(SessionLookupError::Missing) => match server.sessions.create_session() {
                Ok(session_id) => session_id,
                Err(error) => {
                    drop(permit);
                    return session_create_error(id, error);
                }
            },
            Err(error) => {
                drop(permit);
                return session_lookup_error_response(error);
            }
        };
        drop(permit);
        return json_rpc_success_with_session(id, initialize_result(), &session_id);
    }

    if let Err(status) = validate_protocol_version_header(&headers, true) {
        return status.into_response();
    }

    let session = match server.sessions.require_session(&headers) {
        Ok(session) => session,
        Err(error) => return session_lookup_error_response(error),
    };

    if method == "notifications/initialized" {
        if request.id.is_some() {
            return json_rpc_error(id, -32600, "notifications must not include an id");
        }
        let Ok(permit) = admit_mcp_request(&server) else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        if let Err(error) = server.sessions.mark_initialized(session.session_id()) {
            drop(permit);
            return session_lookup_error_response(error);
        }
        drop(permit);
        return StatusCode::ACCEPTED.into_response();
    }

    if !session.initialized {
        return uninitialized_session_response(request.id);
    }

    let namespace = session.namespace();
    if method.starts_with("notifications/") {
        if request.id.is_some() {
            return json_rpc_error(id, -32600, "notifications must not include an id");
        }
        if method == "notifications/cancelled" {
            notifications::handle_notification(&server, method, request.params, &namespace);
            return StatusCode::ACCEPTED.into_response();
        }
        let Ok(permit) = admit_mcp_request(&server) else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        notifications::handle_notification(&server, method, request.params, &namespace);
        drop(permit);
        return StatusCode::ACCEPTED.into_response();
    }

    let Some(id) = request.id else {
        return json_rpc_error(Value::Null, -32600, "requests must include an id");
    };
    let Some(request_id) = request_id_key(&namespace, &id) else {
        return invalid_request_id_response();
    };
    let permit = match admit_mcp_request(&server) {
        Ok(permit) => permit,
        Err(reason) => {
            let error =
                AgentAdapterError::new(AgentAdapterErrorKind::QosRejected, qos_message(reason));
            record_mcp_qos_rejection(&server, method, &id, error.kind.as_str());
            server.metrics.record_rejection("mcp", error.kind.as_str());
            return if method == "tools/call" {
                json_rpc_success(id, tool_error_result(error))
            } else {
                json_rpc_error(id, -32000, error.to_string())
            };
        }
    };

    let started = Instant::now();
    let mut pending_tool_audit = None;
    let result = match method {
        "ping" => json!({}),
        "tools/list" => metrics::tools_list_result(&server, &session),
        "resources/list" => resources::list_resources(&server),
        "resources/read" => {
            match resources::read_resource_with_timeout(&server, request.params, &request_id).await
            {
                Ok(result) => result,
                Err(error) => return json_rpc_error(id, error.code, error.message),
            }
        }
        "prompts/list" => prompts::list_prompts(),
        "prompts/get" => match prompts::get_prompt(&server, request.params, &request_id).await {
            Ok(result) => result,
            Err(error) => return json_rpc_error(id, error.code, error.message),
        },
        "tools/call" => {
            let params = match serde_json::from_value::<ToolCallParams>(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return json_rpc_error(
                        id,
                        -32602,
                        format!("invalid tools/call params: {error}"),
                    );
                }
            };
            if !is_known_tool(&params.name) {
                return json_rpc_error(id, -32602, "unknown tool name");
            }
            let outcome = run_cancellable_tool_call(&server, params, request_id).await;
            pending_tool_audit = Some((
                outcome.operation,
                outcome.request_id,
                outcome.result.clone(),
                outcome.duration_ms,
            ));
            outcome.result
        }
        _ => return json_rpc_error(id, -32601, "method not found"),
    };

    drop(permit);
    if let Some((operation, request_id, result, duration_ms)) = pending_tool_audit {
        record_mcp_tool_audit(&server, &operation, &request_id, &result, duration_ms).await;
    } else if !matches!(method, "resources/read" | "prompts/get") {
        server
            .metrics
            .record_request("mcp", method, "completed", elapsed_millis(started), false);
    }
    json_rpc_success(id, result)
}

async fn handle_mcp_delete(State(server): State<McpServer>, headers: HeaderMap) -> Response {
    if let Err(status) = validate_origin(&server, &headers) {
        return status.into_response();
    }
    if let Err(status) = validate_protocol_version_header(&headers, true) {
        return status.into_response();
    }
    let permit = match admit_mcp_request(&server) {
        Ok(permit) => permit,
        Err(_) => {
            server.metrics.record_rejection("mcp", "qos_rejected");
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
    };
    match server.sessions.terminate_session(&headers) {
        Ok(()) => {
            drop(permit);
            StatusCode::ACCEPTED.into_response()
        }
        Err(error) => {
            drop(permit);
            session_lookup_error_response(error)
        }
    }
}

fn admit_mcp_request(server: &McpServer) -> Result<QosPermit, RejectReason> {
    let policy = server.network.current().qos;
    if crate::net::http::qos_request_context_active() {
        return Ok(QosPermit::already_admitted(server.qos.clone()));
    }
    server.qos.admit_queued_request(&policy)
}

async fn run_cancellable_tool_call(
    server: &McpServer,
    params: ToolCallParams,
    request_id: String,
) -> ToolCallOutcome {
    let started = Instant::now();
    let operation = params.name.clone();
    let (mut cancellation, _registration) = server.cancellations.register(request_id.clone());
    let timeout = Duration::from_millis(server.agent.access_policy.max_runtime_ms);
    let tool = run_tool_call(server, params, request_id.clone());

    let result = tokio::select! {
        result = tokio::time::timeout(timeout, tool) => match result {
            Ok(value) => value,
            Err(_) => {
                server.qos.record_timed_out();
                tool_error_result(AgentAdapterError::new(
                    AgentAdapterErrorKind::Timeout,
                    "MCP tool call exceeded max_runtime_ms",
                ))
            }
        },
        _ = wait_for_cancellation(&mut cancellation) => {
            tool_error_result(AgentAdapterError::new(
                AgentAdapterErrorKind::Cancelled,
                "MCP tool call was cancelled",
            ))
        }
    };

    ToolCallOutcome {
        operation,
        request_id,
        result,
        duration_ms: elapsed_millis(started),
    }
}

struct ToolCallOutcome {
    operation: String,
    request_id: String,
    result: Value,
    duration_ms: u64,
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    while cancellation.changed().await.is_ok() {
        if *cancellation.borrow() {
            return;
        }
    }

    std::future::pending::<()>().await;
}

async fn run_tool_call(server: &McpServer, params: ToolCallParams, request_id: String) -> Value {
    match params.name.as_str() {
        RETRIEVE_CONTEXT_TOOL => retrieve_context_tool(server, params.arguments, request_id).await,
        INSPECT_GRAPH_TOOL => inspect_graph_tool(server, params.arguments, request_id).await,
        HEALTH_TOOL => health_tool(server, request_id).await,
        SERVICE_STATUS_TOOL => service_status_tool(server, request_id).await,
        INDEX_STATUS_TOOL => index_status_tool(server, request_id).await,
        CODE_QUERY_TOOL
        | CODE_CONTEXT_TOOL
        | CODE_FEATURE_FLAGS_TOOL
        | CODE_IMPACT_TOOL
        | CODE_REPOSITORY_SET_QUERY_TOOL
        | CODE_SOFTWARE_QUERY_TOOL
        | CODEBASE_VIEW_TOOL => {
            run_code_tool(server, params.name.as_str(), params.arguments, request_id).await
        }
        _ => json!({
            "content": [{"type": "text", "text": "unknown MCP tool"}],
            "isError": true
        }),
    }
}

async fn retrieve_context_tool(server: &McpServer, arguments: Value, request_id: String) -> Value {
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

async fn inspect_graph_tool(server: &McpServer, arguments: Value, request_id: String) -> Value {
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

async fn health_tool(server: &McpServer, request_id: String) -> Value {
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

async fn service_status_tool(server: &McpServer, request_id: String) -> Value {
    match server
        .service
        .read_only_service_status(request_context(request_id))
        .await
    {
        Ok(response) => tool_success_result("service status loaded", json!(response)),
        Err(error) => api_error_result(error),
    }
}

async fn index_status_tool(server: &McpServer, request_id: String) -> Value {
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

fn endpoint_child(endpoint: &str, child: &str) -> String {
    if endpoint == "/" {
        format!("/{child}")
    } else {
        format!("{}/{child}", endpoint.trim_end_matches('/'))
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn qos_message(reason: RejectReason) -> &'static str {
    match reason {
        RejectReason::ConnectionBudgetExceeded => "connection budget exhausted",
        RejectReason::RequestBudgetExceeded => "request budget exhausted",
        RejectReason::QueueBudgetExceeded => "queue budget exhausted",
    }
}

#[cfg(test)]
#[path = "feature_flag_tool_tests.rs"]
mod feature_flag_tool_tests;
#[cfg(test)]
#[path = "protocol_tests.rs"]
mod protocol_tests;
#[cfg(test)]
#[path = "runtime_guardrail_tests.rs"]
mod runtime_guardrail_tests;
#[cfg(test)]
#[path = "software_tool_tests.rs"]
mod software_tool_tests;
#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "tool_tests.rs"]
mod tool_tests;
