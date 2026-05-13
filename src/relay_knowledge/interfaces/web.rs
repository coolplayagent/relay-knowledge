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
    routing::get,
};
use serde_json::json;

use crate::{
    api::{ApiError, ErrorKind, InterfaceKind, RequestContext},
    application::RelayKnowledgeService,
};

/// Builds the Web router without opening sockets.
pub fn router(service: RelayKnowledgeService) -> Router {
    router_with_assets(service, default_web_dist())
}

fn router_with_assets(service: RelayKnowledgeService, asset_root: PathBuf) -> Router {
    let state = WebState {
        service,
        asset_root: Arc::new(asset_root),
    };

    Router::new()
        .route("/api/project/status", get(project_status))
        .route("/api/health", get(health))
        .route("/api/service/status", get(service_status))
        .route("/", get(index))
        .route("/{*path}", get(asset_or_index))
        .with_state(state)
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
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    use crate::{
        application::RelayKnowledgeService,
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

        let _router = router(service);
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
        let response = router_with_assets(service, root)
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

        router_with_assets(test_service(label).await, root)
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
