use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
        CodeRepositorySetQueryRequest, CodeRetrievalLayer, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn repository_set_query_merges_real_member_scopes_without_collapsing_same_names() {
    let app_repo = FixtureRepo::create("repo-set-app");
    app_repo.write(
        "src/client.rs",
        r#"
use service::serve;

pub fn client() {
    serve();
}

pub fn shared_name() -> u32 {
    1
}
"#,
    );
    app_repo.git(["add", "."]);
    app_repo.git(["commit", "-m", "app"]);
    let service_repo = FixtureRepo::create("repo-set-service");
    service_repo.write(
        "src/service.rs",
        r#"
pub fn serve() -> u32 {
    2
}

pub fn shared_name() -> u32 {
    3
}
"#,
    );
    service_repo.git(["add", "."]);
    service_repo.git(["commit", "-m", "service"]);
    let service = service_with_memory_store().await;

    register_and_index(&service, &app_repo, "app").await;
    register_and_index(&service, &service_repo, "svc").await;
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            context("set-create"),
        )
        .await
        .expect("set should create");
    add_member(&service, "workspace", "app", 10).await;
    add_member(&service, "workspace", "svc", 0).await;

    let response = service
        .query_code_repository_set(
            CodeRepositorySetQueryRequest::new(
                "workspace",
                "shared_name",
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
                Vec::new(),
                Vec::new(),
            )
            .expect("query request should validate"),
            context("set-query"),
        )
        .await
        .expect("set query should succeed");
    let mut repositories = response
        .results
        .iter()
        .filter(|result| result.hit.path.ends_with(".rs"))
        .map(|result| result.member.repository_alias.as_str())
        .collect::<Vec<_>>();
    repositories.sort_unstable();
    repositories.dedup();

    assert_eq!(response.status.members.len(), 2);
    assert!(repositories.contains(&"app"));
    assert!(repositories.contains(&"svc"));
    assert!(
        response
            .results
            .iter()
            .all(|result| !result.hit.scope_id.is_empty())
    );
}

#[tokio::test]
async fn repository_set_refresh_exposes_import_overlay_without_fact_duplication() {
    let app_repo = FixtureRepo::create("repo-set-overlay-app");
    app_repo.write(
        "src/client.rs",
        r#"
use service::serve;

pub fn client() {
    serve();
}
"#,
    );
    app_repo.git(["add", "."]);
    app_repo.git(["commit", "-m", "app"]);
    let service_repo = FixtureRepo::create("repo-set-overlay-service");
    service_repo.write(
        "src/service.rs",
        r#"
pub fn serve() -> u32 {
    2
}
"#,
    );
    service_repo.git(["add", "."]);
    service_repo.git(["commit", "-m", "service"]);
    let service = service_with_memory_store().await;

    register_and_index(&service, &app_repo, "app").await;
    register_and_index(&service, &service_repo, "svc").await;
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            context("overlay-set-create"),
        )
        .await
        .expect("set should create");
    add_member(&service, "workspace", "app", 0).await;
    add_member(&service, "workspace", "svc", 0).await;
    let before = service
        .code_repository_set_status("workspace".to_owned(), context("before-refresh"))
        .await
        .expect("status should load");

    let refreshed = service
        .refresh_code_repository_set("workspace".to_owned(), context("refresh"))
        .await
        .expect("refresh should succeed");
    let import_query = service
        .query_code_repository_set(
            CodeRepositorySetQueryRequest::new(
                "workspace",
                "service::serve",
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
                Vec::new(),
                Vec::new(),
            )
            .expect("query request should validate"),
            context("import-query"),
        )
        .await
        .expect("import query should succeed");

    assert_eq!(before.status.overlay.edge_count, 0);
    assert!(
        refreshed
            .summary
            .as_ref()
            .is_some_and(|summary| summary.edge_count > 0)
    );
    assert!(
        refreshed
            .summary
            .as_ref()
            .is_some_and(|summary| summary.resolved_edge_count > 0)
    );
    assert!(import_query.results.iter().any(|result| {
        result
            .overlay_evidence
            .iter()
            .any(|edge| edge.resolution_state == "resolved")
    }));
    assert_eq!(
        refreshed
            .status
            .members
            .iter()
            .map(|member| member.indexed_file_count)
            .sum::<usize>(),
        2
    );
}

#[tokio::test]
async fn repository_set_query_uses_persisted_member_scope_after_default_filter_change() {
    let repo = FixtureRepo::create("repo-set-persisted-scope");
    repo.write(
        "src/api/public.rs",
        r#"
pub fn shared_target() -> u32 {
    1
}
"#,
    );
    repo.write(
        "src/internal/hidden.rs",
        r#"
pub fn hidden_target() -> u32 {
    2
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_with_filters_and_index(&service, &repo, "app", vec!["src/api"]).await;
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            context("persisted-scope-set-create"),
        )
        .await
        .expect("set should create");
    add_member(&service, "workspace", "app", 0).await;
    register_repository(&service, &repo, "app", vec!["src/internal"]).await;

    let response = service
        .query_code_repository_set(
            CodeRepositorySetQueryRequest::new(
                "workspace",
                "shared_target",
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
                Vec::new(),
                Vec::new(),
            )
            .expect("query request should validate"),
            context("persisted-scope-query"),
        )
        .await
        .expect("set query should use stored member scope");

    assert!(!response.results.is_empty());
    assert!(response.results.iter().all(|result| {
        result.member.path_filters == vec!["src/api".to_owned()]
            && result.hit.path.starts_with("src/api/")
    }));
}

#[tokio::test]
async fn repository_set_query_path_filters_narrow_member_scope() {
    let repo = FixtureRepo::create("repo-set-query-filter");
    repo.write(
        "src/api/public.rs",
        r#"
pub fn shared_target() -> u32 {
    1
}
"#,
    );
    repo.write(
        "src/internal/hidden.rs",
        r#"
pub fn shared_target() -> u32 {
    2
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_with_filters_and_index(&service, &repo, "app", vec!["src"]).await;
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            context("narrow-set-create"),
        )
        .await
        .expect("set should create");
    add_member(&service, "workspace", "app", 0).await;

    let response = service
        .query_code_repository_set(
            CodeRepositorySetQueryRequest::new(
                "workspace",
                "shared_target",
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
                vec!["src/api".to_owned()],
                Vec::new(),
            )
            .expect("query request should validate"),
            context("narrow-query"),
        )
        .await
        .expect("set query should narrow paths");

    assert!(!response.results.is_empty());
    assert!(
        response
            .results
            .iter()
            .all(|result| result.hit.path.starts_with("src/api/"))
    );
}

#[tokio::test]
async fn repository_set_status_marks_moving_member_stale_when_ref_advances() {
    let repo = FixtureRepo::create("repo-set-moving-ref");
    repo.write(
        "src/lib.rs",
        r#"
pub fn original() -> u32 {
    1
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_and_index(&service, &repo, "app").await;
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            context("moving-set-create"),
        )
        .await
        .expect("set should create");
    add_member(&service, "workspace", "app", 0).await;
    repo.write(
        "src/lib.rs",
        r#"
pub fn original() -> u32 {
    2
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "advance"]);

    let response = service
        .code_repository_set_status("workspace".to_owned(), context("moving-status"))
        .await
        .expect("status should load");

    assert_eq!(response.status.freshness_state, "stale");
    assert!(response.status.members[0].stale);
    assert!(
        response.status.members[0]
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("now resolves"))
    );
}

#[tokio::test]
async fn repository_set_import_query_reports_external_dependency_source_fallback() {
    let repo = FixtureRepo::create("repo-set-external-import");
    repo.write(
        "src/lib.rs",
        r#"
use serde::Serialize;

#[derive(Serialize)]
pub struct Event {
    pub payload: String,
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "external rust import"]);
    let service = service_with_memory_store().await;

    register_and_index(&service, &repo, "app").await;
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            context("external-set-create"),
        )
        .await
        .expect("set should create");
    add_member(&service, "workspace", "app", 0).await;
    service
        .refresh_code_repository_set("workspace".to_owned(), context("external-set-refresh"))
        .await
        .expect("set refresh should succeed");

    let response = service
        .query_code_repository_set(
            CodeRepositorySetQueryRequest::new(
                "workspace",
                "serde",
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
                Vec::new(),
                vec!["rust".to_owned()],
            )
            .expect("query request should validate"),
            context("external-import-set-query"),
        )
        .await
        .expect("repository-set query should succeed");

    assert_eq!(response.degraded_reason, None);
    assert!(
        response.results.iter().any(|result| {
            result.member.repository_alias == "app"
                && result.hit.edge_kind.as_deref() == Some("import")
                && result.hit.edge_resolution_state.as_deref() == Some("unresolved")
        }),
        "expected unresolved import graph evidence: {:?}",
        response.results
    );
    assert!(
        response.results.iter().any(|result| {
            result.member.repository_alias == "app"
                && result.hit.excerpt.contains("serde")
                && result
                    .hit
                    .retrieval_layers
                    .contains(&CodeRetrievalLayer::TextFallback)
        }),
        "expected current-repository text fallback evidence: {:?}",
        response.results
    );
}

async fn register_and_index(service: &RelayKnowledgeService, repo: &FixtureRepo, alias: &str) {
    register_with_filters_and_index(service, repo, alias, vec!["src"]).await;
}

async fn register_with_filters_and_index(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    alias: &str,
    path_filters: Vec<&str>,
) {
    register_repository(service, repo, alias, path_filters).await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector(alias),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context(&format!("index-{alias}")),
        )
        .await
        .expect("repository should index");
}

async fn register_repository(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    alias: &str,
    path_filters: Vec<&str>,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: alias.to_owned(),
                path_filters: path_filters.into_iter().map(str::to_owned).collect(),
                language_filters: vec!["rust".to_owned()],
            },
            context(&format!("register-{alias}")),
        )
        .await
        .expect("repository should register");
}

async fn add_member(
    service: &RelayKnowledgeService,
    set_alias: &str,
    repository_alias: &str,
    priority: i32,
) {
    service
        .add_code_repository_set_member(
            CodeRepositorySetAddMemberRequest::new(
                set_alias,
                repository_alias,
                "HEAD",
                Vec::new(),
                Vec::new(),
                priority,
            )
            .expect("member request should validate"),
            context(&format!("add-{repository_alias}")),
        )
        .await
        .expect("member should add");
}

fn selector(alias: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate")
}

fn context(name: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{name}"),
        format!("trace-{name}"),
    )
}

async fn service_with_memory_store() -> RelayKnowledgeService {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    RelayKnowledgeService::with_store(runtime, store)
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
        fs::create_dir_all(path.join("src")).expect("repo directory should be created");
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

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}
