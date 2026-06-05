use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    api::{ApiError, InterfaceKind, RequestContext},
    domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, CodeRetrievalRequest},
};

use super::{WebState, api_error_response};

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route(
            "/api/v1/code/repositories/{alias}/index",
            post(code_repository_index),
        )
        .route(
            "/api/v1/code/repositories/{alias}/scope/preview",
            post(code_repository_scope_preview),
        )
        .route(
            "/api/v1/code/repositories/{alias}/query",
            post(code_repository_query),
        )
        .route(
            "/api/v1/code/repositories/{alias}/status",
            get(code_repository_status),
        )
}

async fn code_repository_index(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Json(mut request): Json<CodeIndexRequest>,
) -> Response {
    if let Some(error) = normalize_selector(&mut request.repository) {
        return api_error_response(error);
    }
    if let Some(error) = path_alias_error(&alias, &request.repository) {
        return api_error_response(error);
    }
    if request.mode != CodeIndexMode::Full {
        return api_error_response(ApiError::invalid_argument(
            "remote code repository index API accepts only full index mode",
        ));
    }
    match state
        .service
        .start_code_repository_index(request, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn code_repository_scope_preview(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Json(mut request): Json<CodeIndexRequest>,
) -> Response {
    if let Some(error) = normalize_selector(&mut request.repository) {
        return api_error_response(error);
    }
    if let Some(error) = path_alias_error(&alias, &request.repository) {
        return api_error_response(error);
    }
    match state
        .service
        .preview_code_repository_scope(request, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn code_repository_query(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Json(mut request): Json<CodeRetrievalRequest>,
) -> Response {
    if let Some(error) = normalize_query_request(&mut request) {
        return api_error_response(error);
    }
    if let Some(error) = path_alias_error(&alias, &request.repository) {
        return api_error_response(error);
    }
    match state
        .service
        .query_code_repository(request, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn code_repository_status(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    Query(query): Query<CodeRepositoryStatusQuery>,
    headers: HeaderMap,
) -> Response {
    let selector = match CodeRepositorySelector::new(
        alias,
        query.ref_selector.unwrap_or_else(|| "HEAD".to_owned()),
        Vec::new(),
        Vec::new(),
    ) {
        Ok(selector) => selector,
        Err(error) => return api_error_response(ApiError::invalid_argument(error.to_string())),
    };
    match state
        .service
        .code_repository_status(selector, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

#[derive(Debug, Deserialize)]
struct CodeRepositoryStatusQuery {
    #[serde(rename = "ref")]
    ref_selector: Option<String>,
}

fn api_context(headers: &HeaderMap) -> RequestContext {
    let generated = RequestContext::for_interface(InterfaceKind::Api);
    RequestContext::with_ids(
        InterfaceKind::Api,
        header_text(headers, "x-relay-request-id").unwrap_or(generated.request_id),
        header_text(headers, "x-relay-trace-id").unwrap_or(generated.trace_id),
    )
}

fn header_text(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_query_request(request: &mut CodeRetrievalRequest) -> Option<ApiError> {
    if let Some(error) = normalize_selector(&mut request.repository) {
        return Some(error);
    }
    match CodeRetrievalRequest::new(
        std::mem::take(&mut request.query),
        request.repository.clone(),
        request.code_query_kind,
        request.limit,
        request.freshness_policy,
    ) {
        Ok(validated) => {
            *request = validated;
            None
        }
        Err(error) => Some(ApiError::invalid_argument(error.to_string())),
    }
}

fn normalize_selector(selector: &mut CodeRepositorySelector) -> Option<ApiError> {
    match CodeRepositorySelector::new(
        std::mem::take(&mut selector.repository),
        std::mem::take(&mut selector.ref_selector),
        std::mem::take(&mut selector.path_filters),
        std::mem::take(&mut selector.language_filters),
    ) {
        Ok(validated) => {
            *selector = validated;
            None
        }
        Err(error) => Some(ApiError::invalid_argument(error.to_string())),
    }
}

fn path_alias_error(path_alias: &str, selector: &CodeRepositorySelector) -> Option<ApiError> {
    if selector.repository == path_alias {
        return None;
    }

    Some(ApiError::invalid_argument(format!(
        "path alias '{path_alias}' must match request repository '{}'",
        selector.repository
    )))
}
