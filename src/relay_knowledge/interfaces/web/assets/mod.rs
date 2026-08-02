//! Static Web asset resolution and response encoding.

use std::path::{Component, Path, PathBuf};

use axum::Json;
use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use super::WebState;

pub(super) async fn index(State(state): State<WebState>) -> Response {
    serve_file_or_status(index_path(&state.asset_root), StatusCode::NOT_FOUND).await
}

pub(super) async fn asset_or_index(
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

pub(super) fn default_web_dist() -> PathBuf {
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
