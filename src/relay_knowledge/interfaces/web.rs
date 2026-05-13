//! Web HTTP adapter for same-origin diagnostics and static assets.

use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::limit::RequestBodyLimitLayer;

use crate::{
    api::{
        ApiError, AuditQueryApiRequest, CodeRepositoryRegisterRequest, ErrorKind,
        GraphInspectionRequest, HybridRetrievalRequest, IndexRefreshRequest, IngestEvidence,
        IngestRequest, InterfaceKind, ProposalDecisionApiRequest, ProposalListApiRequest,
        RequestContext, WorkerRunRequest, WorkerStatusRequest,
    },
    application::RelayKnowledgeService,
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy, IndexKind, ProposalState, WorkerKind,
    },
};

/// Builds the Web router without opening sockets.
pub fn router(service: RelayKnowledgeService, max_request_body_bytes: u64) -> Router {
    router_with_assets(service, default_web_dist(), max_request_body_bytes)
}

fn router_with_assets(
    service: RelayKnowledgeService,
    asset_root: PathBuf,
    max_request_body_bytes: u64,
) -> Router {
    let state = WebState {
        service,
        asset_root: Arc::new(asset_root),
    };
    let body_limit = usize::try_from(max_request_body_bytes).unwrap_or(usize::MAX);

    Router::new()
        .route("/api/project/status", get(project_status))
        .route("/api/health", get(health))
        .route("/api/service/status", get(service_status))
        .route("/api/web/operations/execute", post(execute_operation))
        .route("/", get(index))
        .route("/{*path}", get(asset_or_index))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(body_limit))
}

async fn project_status(State(state): State<WebState>) -> Response {
    match state
        .service
        .project_status(RequestContext::for_interface(InterfaceKind::Web))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn health(State(state): State<WebState>) -> Response {
    match state
        .service
        .health(RequestContext::for_interface(InterfaceKind::Web))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn service_status(State(state): State<WebState>) -> Response {
    match state
        .service
        .service_status(RequestContext::for_interface(InterfaceKind::Web))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn execute_operation(
    State(state): State<WebState>,
    Json(request): Json<ExecuteOperationRequest>,
) -> Result<Response, WebError> {
    let operation = string_field(&request.snapshot.payload, "operation")?;
    let context = RequestContext::for_interface(InterfaceKind::Web);
    let (metadata, result) = dispatch_operation(
        &state.service,
        operation,
        &request.snapshot.payload,
        context,
    )
    .await?;
    let response = ExecuteOperationResponse {
        metadata,
        operation: operation.to_owned(),
        name: request.snapshot.name,
        command: request.snapshot.command,
        result,
    };

    Ok(Json(response).into_response())
}

async fn dispatch_operation(
    service: &RelayKnowledgeService,
    operation: &str,
    payload: &Value,
    context: RequestContext,
) -> Result<(crate::api::ApiMetadata, Value), WebError> {
    match operation {
        "retrieve.context" => {
            let response = service
                .retrieve_context(retrieve_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "graph.ingest" => {
            let response = service.ingest(ingest_request(payload)?, context).await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "graph.inspect" => {
            let response = service
                .inspect_graph(graph_request(payload), context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "index.refresh" => {
            let response = service
                .refresh_indexes(index_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "service.doctor" | "service.run.streamable_http" => {
            let response = service.service_status(context).await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "provider.embedding.probe" => {
            let response = service.probe_embedding_provider(context).await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "worker.status" => {
            let response = service
                .worker_status(
                    WorkerStatusRequest {
                        kind: optional_worker_kind(payload)?,
                    },
                    context,
                )
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "worker.run-once" => {
            let response = service
                .run_worker_once(
                    WorkerRunRequest {
                        kind: optional_worker_kind(payload)?,
                    },
                    context,
                )
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "proposal.list" => {
            let response = service
                .list_proposals(
                    ProposalListApiRequest {
                        state: optional_proposal_state(payload)?,
                        limit: usize_field(payload, "limit")?,
                    },
                    context,
                )
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "proposal.show" => {
            let response = service
                .show_proposal(string_field(payload, "proposal_id")?.to_owned(), context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "proposal.accept" => {
            let response = service
                .accept_proposal(
                    string_field(payload, "proposal_id")?.to_owned(),
                    proposal_decision_request(payload)?,
                    context,
                )
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "proposal.reject" => {
            let response = service
                .decide_proposal_without_commit(
                    string_field(payload, "proposal_id")?.to_owned(),
                    ProposalState::Rejected,
                    proposal_decision_request(payload)?,
                    context,
                )
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "proposal.supersede" => {
            let response = service
                .decide_proposal_without_commit(
                    string_field(payload, "proposal_id")?.to_owned(),
                    ProposalState::Superseded,
                    proposal_decision_request(payload)?,
                    context,
                )
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "audit.query" => {
            let response = service
                .query_audit(
                    AuditQueryApiRequest {
                        operation: optional_string_field(payload, "filter_operation"),
                        limit: usize_field(payload, "limit")?,
                    },
                    context,
                )
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.register" => {
            let response = service
                .register_code_repository(code_register_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.index" => {
            let response = service
                .index_code_repository(code_index_request(payload, CodeIndexMode::Full)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.update" => {
            let mode = CodeIndexMode::incremental(
                string_field(payload, "base_ref")?,
                string_field(payload, "head_ref")?,
            )
            .map_err(|error| WebError::bad_request(error.to_string()))?;
            let response = service
                .index_code_repository(code_index_request(payload, mode)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.query" => {
            let response = service
                .query_code_repository(code_query_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.impact" => {
            let response = service
                .impact_code_repository(code_impact_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.status" => {
            let response = service
                .code_repository_status(code_selector(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        other => Err(WebError::bad_request(format!(
            "unsupported web operation '{other}'"
        ))),
    }
}

async fn index(State(state): State<WebState>) -> Response {
    serve_file_or_status(index_path(&state.asset_root), StatusCode::NOT_FOUND).await
}

async fn asset_or_index(
    State(state): State<WebState>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, Json(json!({"message": "not found"}))).into_response();
    }

    match sanitized_asset_path(&state.asset_root, &path) {
        Some(asset_path)
            if tokio::fs::metadata(&asset_path)
                .await
                .is_ok_and(|meta| meta.is_file()) =>
        {
            serve_file_or_status(asset_path, StatusCode::NOT_FOUND).await
        }
        _ => serve_file_or_status(index_path(&state.asset_root), StatusCode::NOT_FOUND).await,
    }
}

async fn serve_file_or_status(path: PathBuf, missing_status: StatusCode) -> Response {
    match tokio::fs::read(&path).await {
        Ok(body) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type(&path))],
            Body::from(body),
        )
            .into_response(),
        Err(_) => (
            missing_status,
            Json(json!({"message": "web assets are not built; run ./build.sh"})),
        )
            .into_response(),
    }
}

fn api_error_response(error: ApiError) -> Response {
    let status = match error.error_kind {
        ErrorKind::InvalidArgument => StatusCode::BAD_REQUEST,
        ErrorKind::StorageUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ErrorKind::Timeout => StatusCode::REQUEST_TIMEOUT,
        ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (status, Json(error)).into_response()
}

fn sanitized_asset_path(root: &Path, requested: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for component in Path::new(requested).components() {
        match component {
            Component::Normal(segment) => path.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    Some(path)
}

fn index_path(root: &Path) -> PathBuf {
    root.join("index.html")
}

fn default_web_dist() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("web")
        .join("dist")
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn retrieve_request(payload: &Value) -> Result<HybridRetrievalRequest, WebError> {
    Ok(HybridRetrievalRequest {
        query: string_field(payload, "query")?.to_owned(),
        source_scope: optional_string_field(payload, "source_scope"),
        freshness: parse_freshness(string_field(payload, "freshness")?)?,
        limit: usize_field(payload, "limit")?,
    })
}

fn ingest_request(payload: &Value) -> Result<IngestRequest, WebError> {
    Ok(IngestRequest {
        source_scope: string_field(payload, "source_scope")?.to_owned(),
        evidence: vec![IngestEvidence {
            id: None,
            source_path: None,
            span: None,
            confidence: None,
            status: None,
            content: string_field(payload, "content")?.to_owned(),
            entity_labels: string_array_field(payload, "entity_labels")?,
            extraction: None,
        }],
        relations: Vec::new(),
        claims: Vec::new(),
        events: Vec::new(),
    })
}

fn graph_request(payload: &Value) -> GraphInspectionRequest {
    GraphInspectionRequest {
        source_scope: optional_string_field(payload, "source_scope"),
    }
}

fn index_request(payload: &Value) -> Result<IndexRefreshRequest, WebError> {
    Ok(IndexRefreshRequest {
        kinds: string_array_field(payload, "kinds")?
            .into_iter()
            .map(|kind| parse_index_kind(&kind))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn code_register_request(payload: &Value) -> Result<CodeRepositoryRegisterRequest, WebError> {
    Ok(CodeRepositoryRegisterRequest {
        root_path: string_field(payload, "root_path")?.to_owned(),
        alias: string_field(payload, "alias")?.to_owned(),
        path_filters: optional_string_array_field(payload, "path_filters")?,
        language_filters: optional_string_array_field(payload, "language_filters")?,
    })
}

fn code_index_request(payload: &Value, mode: CodeIndexMode) -> Result<CodeIndexRequest, WebError> {
    Ok(CodeIndexRequest {
        repository: code_selector(payload)?,
        mode,
        freshness_policy: FreshnessPolicy::AllowStale,
    })
}

fn code_query_request(payload: &Value) -> Result<CodeRetrievalRequest, WebError> {
    CodeRetrievalRequest::new(
        string_field(payload, "query")?,
        code_selector(payload)?,
        parse_code_query_kind(string_field(payload, "kind")?)?,
        usize_field(payload, "limit")?,
        parse_freshness(string_field(payload, "freshness")?)?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

fn code_impact_request(payload: &Value) -> Result<CodeImpactRequest, WebError> {
    CodeImpactRequest::new(
        code_selector(payload)?,
        string_field(payload, "base_ref")?,
        string_field(payload, "head_ref")?,
        usize_field(payload, "limit")?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

fn code_selector(payload: &Value) -> Result<CodeRepositorySelector, WebError> {
    CodeRepositorySelector::new(
        string_field(payload, "alias")?,
        optional_string_field(payload, "ref").unwrap_or_else(|| "HEAD".to_owned()),
        optional_string_array_field(payload, "path_filters")?,
        optional_string_array_field(payload, "language_filters")?,
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

fn string_field<'a>(payload: &'a Value, field: &'static str) -> Result<&'a str, WebError> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| WebError::bad_request(format!("{field} is required")))
}

fn optional_string_field(payload: &Value, field: &'static str) -> Option<String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn string_array_field(payload: &Value, field: &'static str) -> Result<Vec<String>, WebError> {
    payload
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| WebError::bad_request(format!("{field} must be an array")))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    WebError::bad_request(format!("{field} contains a non-string value"))
                })
        })
        .collect()
}

fn optional_string_array_field(
    payload: &Value,
    field: &'static str,
) -> Result<Vec<String>, WebError> {
    if payload.get(field).is_none() {
        return Ok(Vec::new());
    }

    string_array_field(payload, field)
}

fn usize_field(payload: &Value, field: &'static str) -> Result<usize, WebError> {
    payload
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| WebError::bad_request(format!("{field} must be a positive integer")))
}

fn parse_freshness(value: &str) -> Result<FreshnessPolicy, WebError> {
    match value {
        "allow-stale" => Ok(FreshnessPolicy::AllowStale),
        "wait-until-fresh" => Ok(FreshnessPolicy::WaitUntilFresh),
        "graph-only" => Ok(FreshnessPolicy::GraphOnly),
        other => Err(WebError::bad_request(format!(
            "unsupported freshness '{other}'"
        ))),
    }
}

fn parse_index_kind(value: &str) -> Result<IndexKind, WebError> {
    match value {
        "bm25" => Ok(IndexKind::Bm25),
        "semantic" => Ok(IndexKind::Semantic),
        "vector" => Ok(IndexKind::Vector),
        other => Err(WebError::bad_request(format!(
            "unsupported index kind '{other}'"
        ))),
    }
}

fn parse_code_query_kind(value: &str) -> Result<CodeQueryKind, WebError> {
    match value {
        "hybrid" => Ok(CodeQueryKind::Hybrid),
        "symbol" => Ok(CodeQueryKind::Symbol),
        "definition" => Ok(CodeQueryKind::Definition),
        "references" => Ok(CodeQueryKind::References),
        "callers" => Ok(CodeQueryKind::Callers),
        "callees" => Ok(CodeQueryKind::Callees),
        "imports" => Ok(CodeQueryKind::Imports),
        other => Err(WebError::bad_request(format!(
            "unsupported code query kind '{other}'"
        ))),
    }
}

fn optional_worker_kind(payload: &Value) -> Result<Option<WorkerKind>, WebError> {
    optional_string_field(payload, "kind")
        .map(|kind| {
            WorkerKind::parse(&kind)
                .map_err(|_| WebError::bad_request(format!("unsupported worker kind '{kind}'")))
        })
        .transpose()
}

fn optional_proposal_state(payload: &Value) -> Result<Option<ProposalState>, WebError> {
    optional_string_field(payload, "state")
        .map(|state| {
            ProposalState::parse(&state)
                .map_err(|_| WebError::bad_request(format!("unsupported proposal state '{state}'")))
        })
        .transpose()
}

fn proposal_decision_request(payload: &Value) -> Result<ProposalDecisionApiRequest, WebError> {
    Ok(ProposalDecisionApiRequest {
        actor: string_field(payload, "actor")?.to_owned(),
        reason: optional_string_field(payload, "reason"),
    })
}

#[derive(Debug, Deserialize)]
struct ExecuteOperationRequest {
    snapshot: WebOperationSnapshot,
}

#[derive(Debug, Deserialize)]
struct WebOperationSnapshot {
    name: String,
    command: String,
    payload: Value,
}

#[derive(Debug, Serialize)]
struct ExecuteOperationResponse {
    metadata: crate::api::ApiMetadata,
    operation: String,
    name: String,
    command: String,
    result: Value,
}

#[derive(Debug)]
struct WebError {
    status: StatusCode,
    message: String,
}

impl WebError {
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }
}

impl From<ApiError> for WebError {
    fn from(error: ApiError) -> Self {
        let status = match error.error_kind {
            ErrorKind::InvalidArgument => StatusCode::BAD_REQUEST,
            ErrorKind::StorageUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };

        Self {
            status,
            message: error.message,
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

#[derive(Clone)]
struct WebState {
    service: RelayKnowledgeService,
    asset_root: Arc<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{Request, StatusCode, header},
    };
    use serde_json::{Value, json};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    use crate::{
        api::{IngestEvidenceExtraction, IngestRequest},
        application::RelayKnowledgeService,
        domain::EvidenceModality,
        env::{EnvironmentConfig, PlatformKind},
    };

    #[test]
    fn rejects_asset_path_traversal() {
        let root = PathBuf::from("/srv/web");

        assert!(sanitized_asset_path(&root, "assets/main.js").is_some());
        assert_eq!(
            sanitized_asset_path(&root, "assets/./main.js"),
            Some(root.join("assets").join("main.js"))
        );
        assert!(sanitized_asset_path(&root, "../secret").is_none());
        assert!(sanitized_asset_path(&root, "/etc/passwd").is_none());
    }

    #[test]
    fn reports_expected_content_types() {
        assert_eq!(
            content_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("assets/main.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("data.json")), "application/json");
        assert_eq!(content_type(Path::new("icon.svg")), "image/svg+xml");
        assert_eq!(content_type(Path::new("module.wasm")), "application/wasm");
        assert_eq!(
            content_type(Path::new("asset.bin")),
            "application/octet-stream"
        );
    }

    #[tokio::test]
    async fn default_router_can_be_constructed() {
        let service = test_service("default-router").await;

        let _router = router(service, crate::net::http::DEFAULT_MAX_BODY_BYTES);
    }

    #[tokio::test]
    async fn api_error_response_maps_stable_status_codes() {
        let invalid = api_error_response(ApiError::invalid_argument("bad"));
        let storage = api_error_response(ApiError::storage_unavailable("down"));
        let timeout = api_error_response(ApiError {
            error_kind: ErrorKind::Timeout,
            message: "slow".to_owned(),
            metadata: None,
        });
        let internal = api_error_response(ApiError {
            error_kind: ErrorKind::Internal,
            message: "broken".to_owned(),
            metadata: None,
        });

        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(storage.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(timeout.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(internal.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn serves_project_health_and_service_status_apis() {
        let router = test_router("api").await;

        let project = get_json(router.clone(), "/api/project/status").await;
        let health = get_json(router.clone(), "/api/health").await;
        let service = get_json(router, "/api/service/status").await;

        assert_eq!(project["project_name"], "relay-knowledge");
        assert_eq!(health["healthy"], true);
        assert_eq!(service["service_name"], "relay-knowledge");
    }

    #[tokio::test]
    async fn serves_static_assets_and_spa_fallback() {
        let router = test_router("assets").await;

        let index = get_text(router.clone(), "/").await;
        let asset = get_text(router.clone(), "/assets/main.js").await;
        let fallback = get_text(router, "/workspace/graph").await;

        assert!(index.contains("<title>relay-knowledge</title>"));
        assert_eq!(asset, "console.log('relay');");
        assert!(fallback.contains("<title>relay-knowledge</title>"));
    }

    #[tokio::test]
    async fn api_misses_do_not_fall_back_to_index() {
        let response = test_router("api-miss")
            .await
            .oneshot(
                Request::builder()
                    .uri("/api/missing")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_text(response).await, "{\"message\":\"not found\"}");
    }

    #[tokio::test]
    async fn missing_web_assets_report_build_guidance() {
        let root = unique_temp_dir("missing-assets");
        let service = test_service("missing-assets").await;
        let response = router_with_assets(service, root, crate::net::http::DEFAULT_MAX_BODY_BYTES)
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(
            response_text(response)
                .await
                .contains("web assets are not built")
        );
    }

    #[tokio::test]
    async fn executes_ingest_through_web_operation_endpoint() {
        let service = test_service("execute-ingest").await;
        let payload = execute_json(
            service,
            json!({
                "snapshot": {
                    "name": "Ingest evidence",
                    "command": "relay-knowledge ingest --source docs --content web",
                    "payload": {
                        "operation": "graph.ingest",
                        "source_scope": "docs",
                        "content": "Web operation endpoint commits evidence",
                        "entity_labels": ["Web"]
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;

        assert_eq!(payload["operation"], "graph.ingest");
        assert_eq!(payload["name"], "Ingest evidence");
        assert_eq!(payload["result"]["receipt"]["evidence_count"], 1);
    }

    #[tokio::test]
    async fn executes_read_and_index_operations_through_shared_service() {
        let service = test_service("execute-read").await;
        let ingest = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Ingest evidence",
                    "command": "relay-knowledge ingest --source docs",
                    "payload": {
                        "operation": "graph.ingest",
                        "source_scope": "docs",
                        "content": "Executable Web operations retrieve graph evidence",
                        "entity_labels": ["Web", "Graph"]
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(ingest["result"]["receipt"]["evidence_count"], 1);

        let retrieve = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Retrieve context",
                    "command": "relay-knowledge query Web",
                    "payload": {
                        "operation": "retrieve.context",
                        "query": "Web operations",
                        "source_scope": "docs",
                        "freshness": "allow-stale",
                        "limit": 5
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(retrieve["operation"], "retrieve.context");
        assert_eq!(retrieve["result"]["results"][0]["source_scope"], "docs");

        let graph = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Inspect graph",
                    "command": "relay-knowledge graph inspect",
                    "payload": {
                        "operation": "graph.inspect",
                        "source_scope": "docs"
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(graph["result"]["graph"]["evidence_count"], 1);

        let index = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Refresh indexes",
                    "command": "relay-knowledge index refresh",
                    "payload": {
                        "operation": "index.refresh",
                        "kinds": ["bm25", "semantic", "vector"]
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(
            index["result"]["indexes"]
                .as_array()
                .expect("indexes")
                .len(),
            3
        );

        let status = execute_json(
            service,
            json!({
                "snapshot": {
                    "name": "Service runtime",
                    "command": "relay-knowledge service doctor",
                    "payload": {
                        "operation": "service.doctor",
                        "allowed_scopes": []
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(status["result"]["service_name"], "relay-knowledge");
    }

    #[tokio::test]
    async fn executes_provider_worker_proposal_and_audit_operations() {
        let service = test_service("execute-ops").await;
        service
            .ingest(
                IngestRequest {
                    source_scope: "docs".to_owned(),
                    evidence: vec![IngestEvidence {
                        id: Some("ev-worker".to_owned()),
                        source_path: None,
                        span: None,
                        confidence: None,
                        status: None,
                        content: "Worker proposal source evidence".to_owned(),
                        entity_labels: Vec::new(),
                        extraction: Some(IngestEvidenceExtraction {
                            modality: EvidenceModality::TextSpan,
                            source_uri: None,
                            source_hash: None,
                            media_hash: None,
                            extractor: Some("web-test".to_owned()),
                            extractor_version: Some("1".to_owned()),
                            observed_at: None,
                            parent_evidence_id: None,
                            layout_region: None,
                            embedding_model: None,
                            embedding_dimension: None,
                            diagnostic: None,
                        }),
                    }],
                    relations: Vec::new(),
                    claims: Vec::new(),
                    events: Vec::new(),
                },
                RequestContext::for_interface(InterfaceKind::Web),
            )
            .await
            .expect("ingest should queue worker tasks");

        let provider = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Probe embedding provider",
                    "command": "relay-knowledge provider probe --format json",
                    "payload": {
                        "operation": "provider.embedding.probe",
                        "input": "probe"
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(provider["operation"], "provider.embedding.probe");
        assert_eq!(provider["result"]["ok"], false);
        assert_eq!(
            provider["result"]["error_code"],
            "remote_embedding_not_configured"
        );

        let worker = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Worker run-once",
                    "command": "relay-knowledge worker run-once --kind embedding --format json",
                    "payload": {
                        "operation": "worker.run-once",
                        "kind": "embedding"
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        let proposal_id = worker["result"]["proposals"][0]["proposal_id"]
            .as_str()
            .expect("proposal id should be present")
            .to_owned();

        let proposals = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Proposal list",
                    "command": "relay-knowledge proposal list --state proposed --format json",
                    "payload": {
                        "operation": "proposal.list",
                        "state": "proposed",
                        "limit": 10
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(
            proposals["result"]["proposals"][0]["proposal_id"],
            proposal_id
        );

        let shown = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Proposal show",
                    "command": format!("relay-knowledge proposal show {proposal_id} --format json"),
                    "payload": {
                        "operation": "proposal.show",
                        "proposal_id": proposal_id
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(shown["result"]["proposal"]["proposal_id"], proposal_id);

        let rejected = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Proposal reject",
                    "command": "relay-knowledge proposal reject --by web",
                    "payload": {
                        "operation": "proposal.reject",
                        "proposal_id": proposal_id,
                        "actor": "web-reviewer",
                        "reason": "covered by web endpoint test"
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(rejected["result"]["proposal"]["state"], "rejected");

        let audit = execute_json(
            service,
            json!({
                "snapshot": {
                    "name": "Audit query",
                    "command": "relay-knowledge audit query --operation worker.run_once --format json",
                    "payload": {
                        "operation": "audit.query",
                        "filter_operation": "worker.run_once",
                        "limit": 10
                    }
                }
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(audit["operation"], "audit.query");
        assert!(
            audit["result"]["events"]
                .as_array()
                .expect("events should be an array")
                .iter()
                .any(|event| event["operation"] == "worker.run_once")
        );
    }

    #[tokio::test]
    async fn web_operation_endpoint_maps_bad_payloads_to_http_errors() {
        let service = test_service("execute-errors").await;
        let missing_query = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Retrieve context",
                    "command": "relay-knowledge query",
                    "payload": {
                        "operation": "retrieve.context",
                        "source_scope": "docs",
                        "freshness": "allow-stale",
                        "limit": 5
                    }
                }
            }),
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert_eq!(missing_query["error"], "query is required");

        let bad_kind = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Refresh indexes",
                    "command": "relay-knowledge index refresh",
                    "payload": {
                        "operation": "index.refresh",
                        "kinds": ["unknown"]
                    }
                }
            }),
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert_eq!(bad_kind["error"], "unsupported index kind 'unknown'");

        let missing_repository = execute_json(
            service.clone(),
            json!({
                "snapshot": {
                    "name": "Code status",
                    "command": "relay-knowledge repo status missing",
                    "payload": {
                        "operation": "code.repo.status",
                        "alias": "missing",
                        "ref": "HEAD"
                    }
                }
            }),
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert_eq!(
            missing_repository["error"],
            "code repository 'missing' is not registered"
        );

        let bad_worker = execute_json(
            service,
            json!({
                "snapshot": {
                    "name": "Worker status",
                    "command": "relay-knowledge worker status --kind unknown",
                    "payload": {
                        "operation": "worker.status",
                        "kind": "unknown"
                    }
                }
            }),
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert_eq!(bad_worker["error"], "unsupported worker kind 'unknown'");
    }

    #[tokio::test]
    async fn web_operation_endpoint_enforces_configured_body_limit() {
        let service = test_service("body-limit").await;
        let router = router(service, 64);
        let request = Request::builder()
            .method("POST")
            .uri("/api/web/operations/execute")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "snapshot": {
                        "name": "Oversized",
                        "command": "relay-knowledge ingest",
                        "payload": {
                            "operation": "graph.ingest",
                            "source_scope": "docs",
                            "content": "this body is intentionally larger than the configured limit",
                            "entity_labels": ["Web"]
                        }
                    }
                })
                .to_string(),
            ))
            .expect("request should build");

        let response = router.oneshot(request).await.expect("request should route");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn request_builders_parse_web_payload_variants() {
        let payload = json!({
            "root_path": "/repo",
            "alias": "relay",
            "ref": "main",
            "path_filters": [" src/ ", "tests"],
            "language_filters": [" rust "],
            "query": "handler",
            "kind": "definition",
            "freshness": "wait-until-fresh",
            "limit": 7,
            "base_ref": "main",
            "head_ref": "feature"
        });

        let registration = code_register_request(&payload).expect("registration");
        assert_eq!(registration.path_filters, ["src/", "tests"]);
        assert_eq!(registration.language_filters, ["rust"]);

        let selector = code_selector(&payload).expect("selector");
        assert_eq!(selector.repository, "relay");
        assert_eq!(selector.ref_selector, "main");

        let query = code_query_request(&payload).expect("query");
        assert_eq!(query.code_query_kind, CodeQueryKind::Definition);
        assert_eq!(query.freshness_policy, FreshnessPolicy::WaitUntilFresh);

        let impact = code_impact_request(&payload).expect("impact");
        assert_eq!(impact.base_ref, "main");
        assert_eq!(impact.head_ref, "feature");

        let index = code_index_request(&payload, CodeIndexMode::Full).expect("index");
        assert!(matches!(index.mode, CodeIndexMode::Full));
    }

    #[test]
    fn payload_validation_reports_actionable_errors() {
        let payload = json!({
            "operation": "retrieve.context",
            "query": " ",
            "freshness": "now",
            "limit": 0,
            "kinds": ["bm25", 42]
        });

        assert_eq!(
            string_field(&payload, "query")
                .expect_err("blank string")
                .message,
            "query is required"
        );
        assert_eq!(
            usize_field(&payload, "limit")
                .expect_err("positive integer")
                .message,
            "limit must be a positive integer"
        );
        assert_eq!(
            string_array_field(&payload, "missing")
                .expect_err("array")
                .message,
            "missing must be an array"
        );
        assert_eq!(
            string_array_field(&payload, "kinds")
                .expect_err("string values")
                .message,
            "kinds contains a non-string value"
        );
        assert_eq!(
            parse_freshness("now").expect_err("freshness").message,
            "unsupported freshness 'now'"
        );
        assert_eq!(
            parse_code_query_kind("impact")
                .expect_err("unsupported web query kind")
                .message,
            "unsupported code query kind 'impact'"
        );
    }

    async fn test_router(label: &str) -> Router {
        let root = unique_temp_dir(label).join("web");
        std::fs::create_dir_all(root.join("assets")).expect("asset dir should be created");
        std::fs::write(
            root.join("index.html"),
            "<!doctype html><title>relay-knowledge</title>",
        )
        .expect("index asset should be written");
        std::fs::write(root.join("assets").join("main.js"), "console.log('relay');")
            .expect("script asset should be written");

        router_with_assets(
            test_service(label).await,
            root,
            crate::net::http::DEFAULT_MAX_BODY_BYTES,
        )
    }

    async fn test_service(label: &str) -> RelayKnowledgeService {
        let home = unique_temp_dir(label);
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::Unix,
            [
                ("HOME", "/tmp"),
                (
                    "RELAY_KNOWLEDGE_HOME",
                    home.as_path().to_str().expect("utf8 path"),
                ),
            ],
        )
        .expect("environment should parse");

        RelayKnowledgeService::from_environment(&environment)
            .await
            .expect("service should initialize")
    }

    async fn execute_json(
        service: RelayKnowledgeService,
        body: Value,
        expected_status: StatusCode,
    ) -> Value {
        let router = router(service, crate::net::http::DEFAULT_MAX_BODY_BYTES);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/web/operations/execute")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");
        assert_eq!(response.status(), expected_status);

        serde_json::from_str(&response_text(response).await).expect("response should be json")
    }

    async fn get_json(router: Router, uri: &str) -> Value {
        let response = router
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");
        assert_eq!(response.status(), StatusCode::OK);

        serde_json::from_str(&response_text(response).await).expect("response should be json")
    }

    async fn get_text(router: Router, uri: &str) -> String {
        let response = router
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");
        assert_eq!(response.status(), StatusCode::OK);

        response_text(response).await
    }

    async fn response_text(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");

        String::from_utf8(bytes.to_vec()).expect("body should be utf8")
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("relay-knowledge-web-{label}-{now}"))
    }
}
