use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

use super::router;
use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::RelayKnowledgeService,
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
};

#[tokio::test]
async fn serves_versioned_code_repository_index_status_and_query_apis() {
    let repo = FixtureRepo::create("web-code-api");
    repo.write(
        "src/lib.rs",
        "pub fn retry_policy() -> &'static str { \"bounded\" }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = test_service("web-code-api").await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.to_string_lossy().into_owned(),
                alias: "fixture".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            RequestContext::for_interface(InterfaceKind::Api),
        )
        .await
        .expect("repository should register");
    let router = router(service.clone(), crate::net::http::DEFAULT_MAX_BODY_BYTES);
    let selector = CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");
    let index_request = CodeIndexRequest {
        repository: selector.clone(),
        mode: CodeIndexMode::Full,
        freshness_policy: FreshnessPolicy::AllowStale,
    };

    let preview = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/scope/preview",
        Some(json!(index_request)),
        StatusCode::OK,
    )
    .await;
    assert_eq!(preview["preview"]["selected_file_count"], 1);

    let incremental = CodeIndexRequest {
        repository: selector.clone(),
        mode: CodeIndexMode::incremental("HEAD~1", "HEAD").expect("refs should validate"),
        freshness_policy: FreshnessPolicy::AllowStale,
    };
    let rejected_incremental = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/index",
        Some(json!(incremental)),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(rejected_incremental["error_kind"], "invalid_argument");
    assert!(
        rejected_incremental["message"]
            .as_str()
            .expect("message should render")
            .contains("full index mode")
    );

    let index = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/index",
        Some(json!(index_request)),
        StatusCode::OK,
    )
    .await;
    if let Some(task_id) = index["task"]["task_id"].as_str() {
        service
            .run_code_index_task_once(
                Some(task_id.to_owned()),
                RequestContext::for_interface(InterfaceKind::Api),
            )
            .await
            .expect("index worker should run");
    }

    let status = request_json(
        router.clone(),
        "GET",
        "/api/v1/code/repositories/fixture/status?ref=HEAD",
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["status"]["alias"], "fixture");
    assert_eq!(status["status"]["indexed_file_count"], 1);

    let blank_query = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/query",
        Some(json!({
            "query": " ",
            "repository": selector.clone(),
            "code_query_kind": "definition",
            "limit": 5,
            "freshness_policy": "allow_stale"
        })),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(blank_query["error_kind"], "invalid_argument");
    assert!(
        blank_query["message"]
            .as_str()
            .expect("message should render")
            .contains("query: must not be empty")
    );

    let zero_limit = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/query",
        Some(json!({
            "query": "retry_policy",
            "repository": selector.clone(),
            "code_query_kind": "definition",
            "limit": 0,
            "freshness_policy": "allow_stale"
        })),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(zero_limit["error_kind"], "invalid_argument");
    assert!(
        zero_limit["message"]
            .as_str()
            .expect("message should render")
            .contains("limit: must be greater than zero")
    );

    let query_request = CodeRetrievalRequest::new(
        "retry_policy",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::WaitUntilFresh,
    )
    .expect("query should validate");
    let query = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/query",
        Some(json!(query_request)),
        StatusCode::OK,
    )
    .await;
    assert_eq!(query["results"][0]["path"], "src/lib.rs");

    let mismatch = request_json(
        router,
        "POST",
        "/api/v1/code/repositories/other/query",
        Some(json!(query_request)),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(mismatch["error_kind"], "invalid_argument");
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

async fn request_json(
    router: Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    expected_status: StatusCode,
) -> Value {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = router
        .oneshot(builder.body(body).expect("request should build"))
        .await
        .expect("router should respond");
    assert_eq!(response.status(), expected_status);

    serde_json::from_str(&response_text(response).await).expect("response should be json")
}

async fn response_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");

    String::from_utf8(bytes.to_vec()).expect("body should be utf8")
}

struct FixtureRepo {
    path: PathBuf,
}

impl FixtureRepo {
    fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(&path).expect("repo directory should be created");
        let repo = Self { path };
        repo.git(["init"]);
        repo.git(["config", "user.email", "relay@example.invalid"]);
        repo.git(["config", "user.name", "Relay Test"]);
        repo
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for FixtureRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("relay-knowledge-web-{label}-{now}"))
}
