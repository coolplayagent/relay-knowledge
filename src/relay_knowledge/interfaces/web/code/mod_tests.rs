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

use super::super::router;
use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::RelayKnowledgeService,
    domain::{
        CodeFeatureFlagRequest, CodeIndexMode, CodeIndexRequest, CodeQueryKind,
        CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy,
        RepositoryGraphNeighborhoodRequest, SoftwareGlobalKind, SoftwareGlobalRequest,
    },
    env::{EnvironmentConfig, PlatformKind},
};

#[tokio::test]
async fn serves_snapshot_scoped_okf_repository_graph_api() {
    let repo = FixtureRepo::create("web-repository-graph-api");
    repo.write(
        "knowledge/research/rates.md",
        "---\ntype: concept\ntitle: 利率\nsources:\n  - id: pbc\n    resource: https://www.pbc.gov.cn/\n---\n\n利率证据。[^pbc]\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = test_service("web-repository-graph-api").await;
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
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new())
                    .expect("selector"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            RequestContext::for_interface(InterfaceKind::Api),
        )
        .await
        .expect("repository should index");
    let request = RepositoryGraphNeighborhoodRequest::new(
        CodeRepositorySelector::new(
            "fixture",
            "HEAD",
            vec!["knowledge/research".to_owned()],
            vec!["markdown".to_owned()],
        )
        .expect("selector"),
        "knowledge/research/rates.md",
        1,
        100,
        200,
    )
    .expect("graph request");

    let graph = request_json(
        router(service, crate::net::http::DEFAULT_MAX_BODY_BYTES),
        "POST",
        "/api/v1/code/repositories/fixture/graph",
        Some(json!(request)),
        StatusCode::OK,
    )
    .await;

    assert_eq!(graph["schema_version"], 1);
    assert_eq!(graph["scope"]["requested_ref"], "HEAD");
    assert!(
        graph["nodes"]
            .as_array()
            .is_some_and(|nodes| nodes.len() == 2)
    );
    assert_eq!(graph["edges"][0]["kind"], "cites_source");
}

#[tokio::test]
async fn serves_versioned_code_repository_index_status_and_query_apis() {
    let repo = FixtureRepo::create("web-code-api");
    repo.write(
        "src/lib.rs",
        "pub fn retry_policy() -> &'static str { \"bounded\" }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let initial_commit = repo.git_text(["rev-parse", "HEAD"]);
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
    let registered_only = request_json(
        router.clone(),
        "GET",
        "/api/v1/code/repositories",
        None,
        StatusCode::OK,
    )
    .await;
    assert!(
        registered_only["repositories"]
            .as_array()
            .expect("repositories should be an array")
            .is_empty()
    );
    let selector = CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");
    let index_request = CodeIndexRequest {
        repository: selector.clone(),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
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
        workspace_detection: Default::default(),
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
            .contains("full or worktree overlay index mode")
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

    let repositories = request_json(
        router.clone(),
        "GET",
        "/api/v1/code/repositories",
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(repositories["repositories"][0]["alias"], "fixture");
    assert!(repositories["repositories"][0]["last_indexed_scope_id"].is_string());

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

    repo.write(
        "src/lib.rs",
        "pub fn retry_policy() -> &'static str { \"bounded\" }\npub fn commit_event_policy() {}\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "commit event"]);
    let updated_commit = repo.git_text(["rev-parse", "HEAD"]);
    let update = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/update",
        Some(json!({})),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        update["task"]["mode"]["incremental"]["base_ref"],
        initial_commit
    );
    assert_eq!(update["task"]["resolved_commit_sha"], updated_commit);
    let task_id = update["task"]["task_id"]
        .as_str()
        .expect("update should queue")
        .to_owned();
    service
        .run_code_index_task_once(
            Some(task_id),
            RequestContext::for_interface(InterfaceKind::Api),
        )
        .await
        .expect("update worker should run");
    let mismatch = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/update",
        Some(json!({"repository": "other"})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(mismatch["error_kind"], "invalid_argument");

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
        selector.clone(),
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

    let blank_feature_flags = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/feature-flags",
        Some(json!({
            "query": " ",
            "repository": selector.clone(),
            "limit": 10,
            "freshness_policy": "allow_stale"
        })),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(blank_feature_flags["error_kind"], "invalid_argument");
    assert!(
        blank_feature_flags["message"]
            .as_str()
            .expect("message should render")
            .contains("query: must not be empty")
    );

    let feature_flags_request =
        CodeFeatureFlagRequest::new(None, selector.clone(), 10, FreshnessPolicy::AllowStale)
            .expect("feature flags request should validate");
    let feature_flags = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/feature-flags",
        Some(json!(feature_flags_request)),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        feature_flags["flags"]
            .as_array()
            .expect("flags array")
            .len(),
        0
    );
    assert_eq!(feature_flags["freshness"]["state"], "fresh");

    let zero_impact_limit = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/impact",
        Some(json!({
            "repository": selector.clone(),
            "base_ref": "HEAD~1",
            "head_ref": "HEAD",
            "limit": 0
        })),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(zero_impact_limit["error_kind"], "invalid_argument");
    assert!(
        zero_impact_limit["message"]
            .as_str()
            .expect("message should render")
            .contains("limit: must be greater than zero")
    );

    let report = request_json(
        router.clone(),
        "GET",
        "/api/v1/code/repositories/fixture/report",
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(report["report"]["alias"], "fixture");

    let zero_software_limit = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/software",
        Some(json!({
            "repository": selector.clone(),
            "kind": "relationships",
            "freshness_policy": "allow_stale",
            "limit": 0
        })),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(zero_software_limit["error_kind"], "invalid_argument");
    assert!(
        zero_software_limit["message"]
            .as_str()
            .expect("message should render")
            .contains("limit: must be greater than zero")
    );

    let software_request = SoftwareGlobalRequest::new(
        selector.clone(),
        SoftwareGlobalKind::Relationships,
        FreshnessPolicy::GraphOnly,
        10,
    )
    .expect("software request should validate");
    let software = request_json(
        router.clone(),
        "POST",
        "/api/v1/code/repositories/fixture/software",
        Some(json!(software_request)),
        StatusCode::OK,
    )
    .await;
    assert_eq!(software["request"]["kind"], "relationships");

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

    fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git output should be utf8")
            .trim()
            .to_owned()
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
