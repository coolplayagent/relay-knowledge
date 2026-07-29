use super::*;

pub(in crate::interfaces::agent::mcp) async fn server_with_env<const N: usize>(
    pairs: [(&str, &str); N],
) -> McpServer {
    server_and_service(pairs).await.0
}

pub(in crate::interfaces::agent::mcp) async fn server_and_service<const N: usize>(
    pairs: [(&str, &str); N],
) -> (McpServer, RelayKnowledgeService) {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    server_and_service_with_store(pairs, store).await
}

pub(in crate::interfaces::agent::mcp) async fn server_and_service_with_store<const N: usize>(
    pairs: [(&str, &str); N],
    store: Arc<dyn crate::storage::KnowledgeStore>,
) -> (McpServer, RelayKnowledgeService) {
    let mut base = vec![
        ("HOME", "/home/alice"),
        ("TMPDIR", "/tmp"),
        ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ("RELAY_KNOWLEDGE_MCP_STREAMABLE_HTTP_ENABLED", "true"),
    ];
    base.extend(pairs);
    let environment =
        EnvironmentConfig::from_pairs(PlatformKind::Unix, base).expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let service = RelayKnowledgeService::with_store(runtime.clone(), store);
    let server = McpServer::new(
        service.clone(),
        runtime.network.clone(),
        runtime.agent.clone(),
    );

    (server, service)
}

pub(in crate::interfaces::agent::mcp) async fn call_mcp(
    router: &mut Router,
    payload: Value,
) -> Value {
    if payload.get("method").and_then(Value::as_str) == Some("initialize") {
        let (status, value) = raw_mcp_request(router, payload, []).await;
        assert_eq!(status, StatusCode::OK);
        return value;
    }

    let session_id = initialize_session(router).await;
    call_mcp_with_session(router, payload, &session_id).await
}

pub(in crate::interfaces::agent::mcp) async fn call_mcp_with_session(
    router: &mut Router,
    payload: Value,
    session_id: &str,
) -> Value {
    let (status, value) =
        raw_mcp_request(router, payload, [(MCP_SESSION_ID_HEADER, session_id)]).await;
    assert_eq!(status, StatusCode::OK);
    value
}

pub(in crate::interfaces::agent::mcp) async fn tool_call(
    router: &mut Router,
    id: &str,
    name: &str,
    arguments: Value,
) -> Value {
    call_mcp(
        router,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        }),
    )
    .await
}

pub(in crate::interfaces::agent::mcp) async fn raw_mcp_request<const N: usize>(
    router: &mut Router,
    payload: Value,
    headers: [(&str, &str); N],
) -> (StatusCode, Value) {
    raw_custom_request(router, "POST", "/mcp", &payload.to_string(), headers).await
}

pub(in crate::interfaces::agent::mcp) async fn raw_mcp_request_without_protocol<const N: usize>(
    router: &mut Router,
    payload: Value,
    headers: [(&str, &str); N],
) -> (StatusCode, Value) {
    let (status, _headers, value) = raw_custom_response_with_protocol_default(
        router,
        "POST",
        "/mcp",
        &payload.to_string(),
        headers,
        HeaderDefaults {
            content_type: true,
            accept: true,
            protocol_version: false,
        },
    )
    .await;
    (status, value)
}

pub(in crate::interfaces::agent::mcp) async fn initialize_session(router: &mut Router) -> String {
    let (status, headers, response) = raw_mcp_response(
        router,
        json!({
            "jsonrpc": "2.0",
            "id": "init",
            "method": "initialize",
            "params": initialize_params()
        }),
        [],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    let session_id = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("initialize should issue a session")
        .to_owned();
    let initialized = raw_mcp_request(
        router,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }),
        [(MCP_SESSION_ID_HEADER, session_id.as_str())],
    )
    .await;
    assert_eq!(initialized.0, StatusCode::ACCEPTED);
    session_id
}

pub(in crate::interfaces::agent::mcp) fn initialize_params() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {
            "name": "relay-knowledge-test",
            "version": "0.1.0"
        }
    })
}

pub(in crate::interfaces::agent::mcp) async fn raw_mcp_response<const N: usize>(
    router: &mut Router,
    payload: Value,
    headers: [(&str, &str); N],
) -> (StatusCode, HeaderMap, Value) {
    raw_custom_response(router, "POST", "/mcp", &payload.to_string(), headers).await
}

pub(in crate::interfaces::agent::mcp) async fn raw_custom_request<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
) -> (StatusCode, Value) {
    raw_custom_request_with_defaults(router, method, uri, body, headers, true, true).await
}

pub(in crate::interfaces::agent::mcp) async fn raw_custom_response<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
) -> (StatusCode, HeaderMap, Value) {
    raw_custom_response_with_defaults(router, method, uri, body, headers, true, true).await
}

pub(in crate::interfaces::agent::mcp) async fn raw_custom_request_with_defaults<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
    default_content_type: bool,
    default_accept: bool,
) -> (StatusCode, Value) {
    let (status, _headers, value) = raw_custom_response_with_protocol_default(
        router,
        method,
        uri,
        body,
        headers,
        HeaderDefaults {
            content_type: default_content_type,
            accept: default_accept,
            protocol_version: true,
        },
    )
    .await;
    (status, value)
}

async fn raw_custom_response_with_defaults<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
    default_content_type: bool,
    default_accept: bool,
) -> (StatusCode, HeaderMap, Value) {
    raw_custom_response_with_protocol_default(
        router,
        method,
        uri,
        body,
        headers,
        HeaderDefaults {
            content_type: default_content_type,
            accept: default_accept,
            protocol_version: true,
        },
    )
    .await
}

#[derive(Clone, Copy)]
struct HeaderDefaults {
    content_type: bool,
    accept: bool,
    protocol_version: bool,
}

async fn raw_custom_response_with_protocol_default<const N: usize>(
    router: &mut Router,
    method: &str,
    uri: &str,
    body: &str,
    headers: [(&str, &str); N],
    defaults: HeaderDefaults,
) -> (StatusCode, HeaderMap, Value) {
    let has_content_type = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"));
    let has_accept = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("accept"));
    let has_protocol_version = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(MCP_PROTOCOL_VERSION_HEADER));
    let mut builder = Request::builder().method(method).uri(uri);
    if defaults.content_type && !has_content_type {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if defaults.accept && !has_accept {
        builder = builder.header(header::ACCEPT, "application/json, text/event-stream");
    }
    if defaults.protocol_version && !has_protocol_version {
        builder = builder.header(MCP_PROTOCOL_VERSION_HEADER, MCP_PROTOCOL_VERSION);
    }
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    let response = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(body.to_owned()))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    };

    (status, headers, value)
}

pub(in crate::interfaces::agent::mcp) fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"]
        .as_array()
        .expect("tools should be an array")
        .iter()
        .map(|tool| {
            tool["name"]
                .as_str()
                .expect("tool should have a name")
                .to_owned()
        })
        .collect()
}
