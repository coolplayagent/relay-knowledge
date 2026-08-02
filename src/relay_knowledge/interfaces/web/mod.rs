//! Web HTTP adapter for same-origin diagnostics and static assets.

mod assets;
mod code;
mod files;
mod model_config;
mod operation_request;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_http::limit::RequestBodyLimitLayer;

use crate::{
    api::{
        ApiError, AuditQueryApiRequest, ErrorKind, GRAPH_CANVAS_DEFAULT_LIMIT, GraphCanvasKind,
        GraphCanvasRequest, InterfaceKind, ProposalListApiRequest, RequestContext,
        WorkerRunRequest, WorkerStatusRequest,
    },
    application::RelayKnowledgeService,
    domain::{CodeIndexMode, ProposalState},
};
use assets::{asset_or_index, default_web_dist, index};
use code::{code_index_request, code_view_request};
use operation_request::{
    code_context_request, code_feature_flag_request, code_impact_request, code_query_request,
    code_register_request, code_repository_set_add_request, code_repository_set_create_request,
    code_repository_set_query_request, code_repository_set_remove_request, code_selector,
    code_software_request, graph_request, index_request, ingest_request, optional_bool_field,
    optional_proposal_state, optional_string_array_field, optional_string_field,
    optional_worker_kind, parse_freshness, proposal_decision_request, retrieve_request,
    string_field, usize_field,
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
        .route("/api/v1/control/status", get(control_status))
        .route("/api/v1/control/health", get(control_health))
        .route(
            "/api/v1/control/service/status",
            get(read_only_service_status),
        )
        .route("/api/v1/control/storage/topology", get(storage_topology))
        .merge(code::routes())
        .route("/api/web/graph/canvas", get(graph_canvas))
        .route("/api/web/operations/execute", post(execute_operation))
        .merge(model_config::routes())
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

async fn control_status(State(state): State<WebState>) -> Response {
    let (response, _) = state
        .service
        .runtime_diagnostics(RequestContext::for_interface(InterfaceKind::Web));

    Json(response).into_response()
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

async fn control_health(State(state): State<WebState>) -> Response {
    match state
        .service
        .read_only_health(RequestContext::for_interface(InterfaceKind::Web))
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

async fn read_only_service_status(State(state): State<WebState>) -> Response {
    match state
        .service
        .read_only_service_status(RequestContext::for_interface(InterfaceKind::Web))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn storage_topology(State(state): State<WebState>) -> Response {
    match state
        .service
        .storage_topology_status(RequestContext::for_interface(InterfaceKind::Web))
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
        "files.index" | "files.query" | "files.content" => {
            files::dispatch_file_operation(service, operation, payload, context).await
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
        "code.repo.context" => {
            let response = service
                .codegraph_context(code_context_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.feature_flags" => {
            let response = service
                .query_code_repository_feature_flags(code_feature_flag_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.impact" => {
            let response = service
                .impact_code_repository(code_impact_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.view" => {
            let response = service
                .codebase_view(code_view_request(payload)?, context)
                .await?;
            Ok((response.metadata.clone(), json!(response)))
        }
        "code.repo.software" => {
            let response = service
                .software_global_projection(code_software_request(payload)?, context)
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
        "code.repo_set.remove" => {
            let response = service
                .remove_code_repository_set_member(
                    code_repository_set_remove_request(payload)?,
                    context,
                )
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
            let response = if optional_bool_field(payload, "async")?.unwrap_or(false) {
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

pub(super) fn api_error_response(error: ApiError) -> Response {
    let status = match error.error_kind {
        ErrorKind::InvalidArgument => StatusCode::BAD_REQUEST,
        ErrorKind::StorageUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ErrorKind::QosRejected => StatusCode::TOO_MANY_REQUESTS,
        ErrorKind::Timeout => StatusCode::REQUEST_TIMEOUT,
        ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (status, Json(error)).into_response()
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
pub(in crate::interfaces) struct WebError {
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
            ErrorKind::QosRejected => StatusCode::TOO_MANY_REQUESTS,
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
#[path = "control_tests.rs"]
mod control_tests;

#[cfg(test)]
#[path = "router_files_integration_tests.rs"]
mod router_files_integration_tests;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
