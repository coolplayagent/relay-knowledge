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
    domain::{
        CodeFeatureFlagRequest, CodeGraphContextRequest, CodeImpactRequest, CodeIndexMode,
        CodeIndexRequest, CodeRepositorySelector, CodeRetrievalRequest, CodebaseViewRequest,
        SoftwareGlobalRequest,
    },
    interfaces::code_index_mode::normalize_index_request,
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
            "/api/v1/code/repositories/{alias}/context",
            post(code_repository_context),
        )
        .route(
            "/api/v1/code/repositories/{alias}/feature-flags",
            post(code_repository_feature_flags),
        )
        .route(
            "/api/v1/code/repositories/{alias}/impact",
            post(code_repository_impact),
        )
        .route(
            "/api/v1/code/repositories/{alias}/report",
            get(code_repository_report),
        )
        .route(
            "/api/v1/code/repositories/{alias}/software",
            post(code_repository_software),
        )
        .route(
            "/api/v1/code/repositories/{alias}/views",
            post(codebase_view),
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
    if !matches!(
        request.mode,
        CodeIndexMode::Full | CodeIndexMode::WorktreeOverlay
    ) {
        return api_error_response(ApiError::invalid_argument(
            "remote code repository index API accepts only full or worktree overlay index mode",
        ));
    }
    let request = normalize_index_request(request);
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

async fn code_repository_context(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Json(mut request): Json<CodeGraphContextRequest>,
) -> Response {
    if let Some(error) = normalize_context_request(&mut request) {
        return api_error_response(error);
    }
    if let Some(error) = path_alias_error(&alias, &request.repository) {
        return api_error_response(error);
    }
    match state
        .service
        .codegraph_context(request, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn code_repository_feature_flags(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Json(mut request): Json<CodeFeatureFlagRequest>,
) -> Response {
    if let Some(error) = normalize_feature_flag_request(&mut request) {
        return api_error_response(error);
    }
    if let Some(error) = path_alias_error(&alias, &request.repository) {
        return api_error_response(error);
    }
    match state
        .service
        .query_code_repository_feature_flags(request, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn code_repository_impact(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Json(mut request): Json<CodeImpactRequest>,
) -> Response {
    if let Some(error) = normalize_impact_request(&mut request) {
        return api_error_response(error);
    }
    if let Some(error) = path_alias_error(&alias, &request.repository) {
        return api_error_response(error);
    }
    match state
        .service
        .impact_code_repository(request, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn code_repository_report(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    let selector = match CodeRepositorySelector::new(alias, "HEAD", Vec::new(), Vec::new()) {
        Ok(selector) => selector,
        Err(error) => return api_error_response(ApiError::invalid_argument(error.to_string())),
    };
    match state
        .service
        .code_repository_report(selector, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn code_repository_software(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Json(mut request): Json<SoftwareGlobalRequest>,
) -> Response {
    if let Some(error) = normalize_software_request(&mut request) {
        return api_error_response(error);
    }
    if let Some(error) = path_alias_error(&alias, &request.repository) {
        return api_error_response(error);
    }
    match state
        .service
        .software_global_projection(request, api_context(&headers))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn codebase_view(
    State(state): State<WebState>,
    AxumPath(alias): AxumPath<String>,
    headers: HeaderMap,
    Json(mut request): Json<CodebaseViewRequest>,
) -> Response {
    if let Some(error) = normalize_view_request(&mut request) {
        return api_error_response(error);
    }
    if let Some(error) = path_alias_error(&alias, &request.repository) {
        return api_error_response(error);
    }
    match state
        .service
        .codebase_view(request, api_context(&headers))
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
    let exclude_generated = request.exclude_generated;
    match CodeRetrievalRequest::new(
        std::mem::take(&mut request.query),
        request.repository.clone(),
        request.code_query_kind,
        request.limit,
        request.freshness_policy,
    ) {
        Ok(mut validated) => {
            validated.exclude_generated = exclude_generated;
            *request = validated;
            None
        }
        Err(error) => Some(ApiError::invalid_argument(error.to_string())),
    }
}

fn normalize_context_request(request: &mut CodeGraphContextRequest) -> Option<ApiError> {
    if let Some(error) = normalize_selector(&mut request.repository) {
        return Some(error);
    }
    match CodeGraphContextRequest::new(
        request.repository.clone(),
        std::mem::take(&mut request.query),
        request.limit,
        request.freshness_policy,
        request.max_context_bytes,
        request.include_code,
        request.exclude_generated,
    ) {
        Ok(validated) => {
            *request = validated;
            None
        }
        Err(error) => Some(ApiError::invalid_argument(error.to_string())),
    }
}

fn normalize_feature_flag_request(request: &mut CodeFeatureFlagRequest) -> Option<ApiError> {
    if let Some(error) = normalize_selector(&mut request.repository) {
        return Some(error);
    }
    match CodeFeatureFlagRequest::new(
        request.query.take(),
        request.repository.clone(),
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

fn normalize_impact_request(request: &mut CodeImpactRequest) -> Option<ApiError> {
    if let Some(error) = normalize_selector(&mut request.repository) {
        return Some(error);
    }
    match CodeImpactRequest::new(
        request.repository.clone(),
        std::mem::take(&mut request.base_ref),
        std::mem::take(&mut request.head_ref),
        request.limit,
    ) {
        Ok(validated) => {
            *request = validated;
            None
        }
        Err(error) => Some(ApiError::invalid_argument(error.to_string())),
    }
}

fn normalize_software_request(request: &mut SoftwareGlobalRequest) -> Option<ApiError> {
    if let Some(error) = normalize_selector(&mut request.repository) {
        return Some(error);
    }
    match SoftwareGlobalRequest::new(
        request.repository.clone(),
        request.kind,
        request.freshness_policy,
        request.limit,
    ) {
        Ok(validated) => {
            *request = validated;
            None
        }
        Err(error) => Some(ApiError::invalid_argument(error.to_string())),
    }
}

fn normalize_view_request(request: &mut CodebaseViewRequest) -> Option<ApiError> {
    if let Some(error) = normalize_selector(&mut request.repository) {
        return Some(error);
    }
    match CodebaseViewRequest::new(
        request.repository.clone(),
        request.view_kind,
        request.freshness_policy,
        request.limit,
        std::mem::take(&mut request.changed_paths),
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
