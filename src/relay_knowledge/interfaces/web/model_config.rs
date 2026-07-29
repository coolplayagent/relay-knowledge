//! Web routes for model provider configuration.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;

use crate::model_provider::{
    ModelConnectivityProbeRequest, ModelDiscoveryRequest, ModelFallbackConfig,
    ModelProfileSaveRequest,
};

use super::{WebState, api_error_response};

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/api/configs/model/profiles", get(model_profiles))
        .route(
            "/api/configs/model/profiles/{name}",
            put(save_model_profile).delete(delete_model_profile),
        )
        .route(
            "/api/configs/model-fallback",
            get(model_fallback).put(save_model_fallback),
        )
        .route("/api/configs/model/catalog", get(model_catalog))
        .route(
            "/api/configs/model/catalog:refresh",
            post(refresh_model_catalog),
        )
        .route("/api/configs/model:probe", post(probe_model))
        .route("/api/configs/model:discover", post(discover_models))
}

async fn model_profiles(State(state): State<WebState>) -> Response {
    match state.service.model_profiles().await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn save_model_profile(
    State(state): State<WebState>,
    Path(name): Path<String>,
    Json(request): Json<ModelProfileSaveRequest>,
) -> Response {
    match state.service.save_model_profile(&name, request).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn delete_model_profile(State(state): State<WebState>, Path(name): Path<String>) -> Response {
    match state.service.delete_model_profile(&name).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn model_fallback(State(state): State<WebState>) -> Response {
    match state.service.model_fallback_config().await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn save_model_fallback(
    State(state): State<WebState>,
    Json(config): Json<ModelFallbackConfig>,
) -> Response {
    match state.service.save_model_fallback_config(config).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

#[derive(Debug, Deserialize)]
struct ModelCatalogQuery {
    refresh: Option<bool>,
}

async fn model_catalog(
    State(state): State<WebState>,
    Query(query): Query<ModelCatalogQuery>,
) -> Response {
    match state
        .service
        .model_catalog(query.refresh.unwrap_or(false))
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn refresh_model_catalog(State(state): State<WebState>) -> Response {
    match state.service.model_catalog(true).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn probe_model(
    State(state): State<WebState>,
    Json(request): Json<ModelConnectivityProbeRequest>,
) -> Response {
    match state.service.probe_model_provider(request).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}

async fn discover_models(
    State(state): State<WebState>,
    Json(request): Json<ModelDiscoveryRequest>,
) -> Response {
    match state.service.discover_model_provider(request).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => api_error_response(error),
    }
}
