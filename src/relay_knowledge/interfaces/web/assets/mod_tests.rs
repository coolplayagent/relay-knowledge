//! Direct tests for static Web asset resolution.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::to_bytes;
use axum::http::StatusCode;

use super::*;

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
async fn missing_asset_reports_build_guidance() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let missing = std::env::temp_dir()
        .join(format!("relay-knowledge-missing-web-asset-{unique}"))
        .join("index.html");

    let response = serve_file_or_status(missing, StatusCode::NOT_FOUND).await;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        String::from_utf8(body.to_vec())
            .expect("response should be utf8")
            .contains("web assets are not built")
    );
}
