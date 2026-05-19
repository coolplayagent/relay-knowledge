//! Web HTTP adapter for same-origin diagnostics and static assets.

#[path = "web_model_config.rs"]
mod web_model_config;

use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::limit::RequestBodyLimitLayer;

use crate::{
    api::{
        ApiError, AuditQueryApiRequest, CodeRepositoryRegisterRequest, ErrorKind, FileIndexRequest,
        FileQueryRequest, GRAPH_CANVAS_DEFAULT_LIMIT, GraphCanvasKind, GraphCanvasRequest,
        GraphInspectionRequest, HybridRetrievalRequest, IndexRefreshRequest, IngestEvidence,
        IngestRequest, InterfaceKind, ProposalDecisionApiRequest, ProposalListApiRequest,
        RequestContext, WorkerRunRequest, WorkerStatusRequest,
    },
    application::RelayKnowledgeService,
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
        CodeRepositorySetQueryRequest, CodeRetrievalRequest, FreshnessPolicy, IndexKind,
        ProposalState, WorkerKind,
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
        .route("/api/web/graph/canvas", get(graph_canvas))
        .route("/api/web/operations/execute", post(execute_operation))
        .merge(web_model_config::routes())
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

async fn graph_canvas(
    State(state): State<WebState>,
    Query(query): Query<GraphCanvasQuery>,
) -> Response {
    let kind = match query
        .kind
        .as_deref()
        .map(GraphCanvasKind::parse)
        .transpose()
    {
        Ok(kind) => kind.unwrap_or(GraphCanvasKind::Knowledge),
        Err(message) => return WebError::bad_request(message).into_response(),
    };
    let request = GraphCanvasRequest {
        kind,
        source_scope: query.scope.and_then(non_empty_query_value),
        query: query.query.and_then(non_empty_query_value),
        limit: query.limit.unwrap_or(GRAPH_CANVAS_DEFAULT_LIMIT),
    };

    match state
        .service
        .graph_canvas(request, RequestContext::for_interface(InterfaceKind::Web))
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
        "files.index" => {
            let response = service
                .index_files(file_index_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "files.query" => {
            let response = service
                .query_files(file_query_request(payload)?, context)
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
                .start_code_repository_index(
                    code_index_request(payload, CodeIndexMode::Full)?,
                    context,
                )
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
        "code.repo_set.create" => {
            let response = service
                .create_code_repository_set(code_repository_set_create_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo_set.add" => {
            let response = service
                .add_code_repository_set_member(code_repository_set_add_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo_set.query" => {
            let response = service
                .query_code_repository_set(code_repository_set_query_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo_set.status" => {
            let response = service
                .code_repository_set_status(string_field(payload, "set_alias")?.to_owned(), context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo_set.refresh" => {
            let set_alias = string_field(payload, "set_alias")?.to_owned();
            let response = if bool_field(payload, "async") {
                service
                    .start_code_repository_set_refresh(set_alias, context)
                    .await?
            } else {
                service
                    .refresh_code_repository_set(set_alias, context)
                    .await?
            };
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

fn file_index_request(payload: &Value) -> Result<FileIndexRequest, WebError> {
    Ok(FileIndexRequest {
        source_scope: optional_string_field(payload, "source_scope"),
        roots: optional_string_array_field(payload, "roots")?,
    })
}

fn file_query_request(payload: &Value) -> Result<FileQueryRequest, WebError> {
    Ok(FileQueryRequest {
        query: string_field(payload, "query")?.to_owned(),
        source_scope: optional_string_field(payload, "source_scope"),
        root_id: optional_string_field(payload, "root_id"),
        limit: usize_field(payload, "limit")?,
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

fn code_repository_set_create_request(
    payload: &Value,
) -> Result<CodeRepositorySetCreateRequest, WebError> {
    CodeRepositorySetCreateRequest::new(
        string_field(payload, "set_alias")?,
        optional_string_field(payload, "description"),
        optional_string_field(payload, "default_ref_policy_json"),
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

fn code_repository_set_add_request(
    payload: &Value,
) -> Result<CodeRepositorySetAddMemberRequest, WebError> {
    CodeRepositorySetAddMemberRequest::new(
        string_field(payload, "set_alias")?,
        string_field(payload, "repository_alias")?,
        string_field(payload, "ref")?,
        optional_string_array_field(payload, "path_filters")?,
        optional_string_array_field(payload, "language_filters")?,
        optional_i32_field(payload, "priority")?.unwrap_or(0),
    )
    .map_err(|error| WebError::bad_request(error.to_string()))
}

fn code_repository_set_query_request(
    payload: &Value,
) -> Result<CodeRepositorySetQueryRequest, WebError> {
    CodeRepositorySetQueryRequest::new(
        string_field(payload, "set_alias")?,
        string_field(payload, "query")?,
        parse_code_query_kind(string_field(payload, "kind")?)?,
        usize_field(payload, "limit")?,
        parse_freshness(string_field(payload, "freshness")?)?,
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

fn i32_field(payload: &Value, field: &'static str) -> Result<i32, WebError> {
    payload
        .get(field)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| WebError::bad_request(format!("{field} must be an integer")))
}

fn optional_i32_field(payload: &Value, field: &'static str) -> Result<Option<i32>, WebError> {
    if payload.get(field).is_none() {
        return Ok(None);
    }

    i32_field(payload, field).map(Some)
}

fn bool_field(payload: &Value, field: &'static str) -> bool {
    payload.get(field).and_then(Value::as_bool).unwrap_or(false)
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
struct GraphCanvasQuery {
    kind: Option<String>,
    scope: Option<String>,
    query: Option<String>,
    limit: Option<usize>,
}

fn non_empty_query_value(value: String) -> Option<String> {
    let trimmed = value.trim();

    (!trimmed.is_empty()).then(|| trimmed.to_owned())
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
pub(super) struct WebState {
    pub(super) service: RelayKnowledgeService,
    asset_root: Arc<PathBuf>,
}

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
