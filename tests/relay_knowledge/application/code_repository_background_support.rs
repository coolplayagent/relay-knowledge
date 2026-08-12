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
    domain::{CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy},
    env::{EnvironmentConfig, PlatformKind},
    interfaces::cli::{CliCommand, run_with_service},
    storage::{KnowledgeStore, SqliteGraphStore},
};

pub(super) async fn drain_code_scope_maintenance(service: &RelayKnowledgeService) {
    for pass in 0..128 {
        let command = CliCommand::parse(["repo", "index-worker", "--format", "json"])
            .expect("local maintenance command should parse");
        let output = run_with_service(service, command, context("drain-scope-maintenance"))
            .await
            .expect("local maintenance command should run");
        let response: serde_json::Value =
            serde_json::from_str(&output).expect("maintenance response should be JSON");
        assert!(
            response.get("maintenance_error").is_none(),
            "bounded scope maintenance failed: {response}"
        );
        if response["maintenance_active"] == false {
            return;
        }
        assert!(pass < 127, "bounded scope maintenance did not converge");
    }
}

pub(super) async fn query(
    service: &RelayKnowledgeService,
    query: &str,
    kind: CodeQueryKind,
) -> relay_knowledge::api::CodeRepositoryQueryResponse {
    query_ref(service, query, "HEAD", kind).await
}

pub(super) async fn query_ref(
    service: &RelayKnowledgeService,
    query: &str,
    ref_selector: &str,
    kind: CodeQueryKind,
) -> relay_knowledge::api::CodeRepositoryQueryResponse {
    service
        .query_code_repository(
            CodeRetrievalRequest::new(
                query,
                selector("fixture", ref_selector),
                kind,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query"),
        )
        .await
        .expect("query should succeed")
}

pub(super) async fn register_fixture_repo(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    name: &str,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context(name),
        )
        .await
        .expect("repository should register");
}

pub(super) fn selector(alias: &str, ref_selector: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, Vec::new(), Vec::new())
        .expect("selector should validate")
}

pub(super) fn filtered_selector(
    alias: &str,
    ref_selector: &str,
    path: &str,
) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, vec![path.to_owned()], Vec::new())
        .expect("selector should validate")
}

pub(super) fn context(name: &str) -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        format!("req-{name}"),
        format!("trace-{name}"),
    )
}

pub(super) async fn service_with_memory_store() -> RelayKnowledgeService {
    service_with_store(Arc::new(
        SqliteGraphStore::open_in_memory().expect("store should open"),
    ))
    .await
}

pub(super) async fn service_with_file_store(name: &str) -> RelayKnowledgeService {
    let path = unique_database_path(name);
    service_with_store(Arc::new(
        SqliteGraphStore::open(path).expect("file store should open"),
    ))
    .await
}

pub(super) async fn service_with_store(store: Arc<dyn KnowledgeStore>) -> RelayKnowledgeService {
    let environment = test_environment();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
}

#[cfg(windows)]
pub(super) fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Windows,
        [
            ("USERPROFILE", "C:\\Users\\alice"),
            ("APPDATA", "C:\\Users\\alice\\AppData\\Roaming"),
            ("LOCALAPPDATA", "C:\\Users\\alice\\AppData\\Local"),
            ("TEMP", "C:\\Users\\alice\\AppData\\Local\\Temp"),
            ("RELAY_KNOWLEDGE_HOME", "C:\\relay"),
        ],
    )
    .expect("environment should parse")
}

#[cfg(not(windows))]
pub(super) fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            (
                "RELAY_KNOWLEDGE_WATCHER_COMMIT_RECONCILE_INTERVAL_MS",
                "100",
            ),
        ],
    )
    .expect("environment should parse")
}

pub(super) struct FixtureRepo {
    pub(super) path: PathBuf,
}

impl FixtureRepo {
    pub(super) fn create(name: &str) -> Self {
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

    pub(super) fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }

    pub(super) fn git<const N: usize>(&self, args: [&str; N]) {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(super) fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = git_command(&self.path, args)
            .output()
            .expect("git should run");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

pub(super) fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}

pub(super) fn unique_database_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    std::env::temp_dir()
        .join("relay-knowledge-tests")
        .join(format!("{name}-{}-{nanos}.sqlite", std::process::id()))
}
