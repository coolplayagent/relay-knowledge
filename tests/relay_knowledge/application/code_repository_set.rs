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
        CodeRepositorySetQueryRequest, FreshnessPolicy,
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

async fn register_and_index(service: &RelayKnowledgeService, repo: &FixtureRepo, alias: &str) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: alias.to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            },
            context(&format!("register-{alias}")),
        )
        .await
        .expect("repository should register");
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
