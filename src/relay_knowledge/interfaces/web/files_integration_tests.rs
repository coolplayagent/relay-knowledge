use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

use crate::{
    application::RelayKnowledgeService,
    env::{EnvironmentConfig, PlatformKind},
};

use super::router;

#[tokio::test]
async fn executes_file_content_operation_through_web_dispatch() {
    let root = unique_temp_dir("execute-files-content-root");
    std::fs::write(root.join("runbook.md"), "service depends on database")
        .expect("file fixture should be written");
    let service = test_service_with_file_root("execute-files-content", &root).await;

    let index = execute_json(
        service.clone(),
        json!({
            "snapshot": {
                "name": "Index local files",
                "command": "relay-knowledge files index",
                "payload": {
                    "operation": "files.index",
                    "source_scope": "local-files",
                    "roots": []
                }
            }
        }),
        StatusCode::OK,
    )
    .await;
    assert_eq!(index["operation"], "files.index");

    let content = execute_json(
        service,
        json!({
            "snapshot": {
                "name": "Search local file content",
                "command": "relay-knowledge files content database",
                "payload": {
                    "operation": "files.content",
                    "query": "database",
                    "source_scope": "local-files",
                    "freshness": "allow-stale",
                    "limit": 5
                }
            }
        }),
        StatusCode::OK,
    )
    .await;

    assert_eq!(content["operation"], "files.content");
    assert_eq!(
        content["result"]["results"][0]["content_role"],
        "user_source"
    );
    assert!(
        content["result"]["results"][0]["path"]
            .as_str()
            .expect("path should be a string")
            .ends_with("runbook.md")
    );
}

async fn test_service_with_file_root(label: &str, root: &Path) -> RelayKnowledgeService {
    let home = unique_temp_dir(label);
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/tmp"),
            (
                "RELAY_KNOWLEDGE_HOME",
                home.as_path().to_str().expect("utf8 path"),
            ),
            (
                "RELAY_KNOWLEDGE_FILE_INDEX_ROOTS",
                root.to_str().expect("utf8 root path"),
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

async fn response_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");

    String::from_utf8(bytes.to_vec()).expect("response should be utf8")
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be valid")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-web-files-{label}-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("temp dir should be created");
    root
}
