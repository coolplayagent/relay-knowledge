use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy},
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

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

pub(super) async fn register_fixture_source(
    service: &RelayKnowledgeService,
    source: &FixtureSourceDir,
    name: &str,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: source.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context(name),
        )
        .await
        .expect("repository should register");
}

pub(super) fn request(alias: &str, ref_selector: &str) -> CodeIndexRequest {
    CodeIndexRequest {
        repository: selector(alias, ref_selector),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
    }
}

pub(super) fn selector(alias: &str, ref_selector: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, Vec::new(), Vec::new())
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
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    service_with_store(store).await
}

pub(super) async fn service_with_store(store: Arc<SqliteGraphStore>) -> RelayKnowledgeService {
    let runtime_root = std::env::temp_dir().join("relay-knowledge-code-repository-runtime");
    let home_dir = runtime_root.join("home");
    let temp_dir = runtime_root.join("tmp");
    let relay_home = runtime_root.join("relay");
    for directory in [&home_dir, &temp_dir, &relay_home] {
        fs::create_dir_all(directory).expect("runtime test directory should be created");
    }
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [
            (OsString::from("HOME"), home_dir.into_os_string()),
            (OsString::from("TEMP"), temp_dir.clone().into_os_string()),
            (OsString::from("TMP"), temp_dir.clone().into_os_string()),
            (OsString::from("TMPDIR"), temp_dir.into_os_string()),
            (
                OsString::from("RELAY_KNOWLEDGE_HOME"),
                relay_home.into_os_string(),
            ),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
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

fn git_command<const N: usize>(path: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path).args(args);
    command
}

pub(super) struct FixtureSourceDir {
    pub(super) path: PathBuf,
}

impl FixtureSourceDir {
    pub(super) fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(path.join("src")).expect("source directory should be created");
        Self { path }
    }

    pub(super) fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }
}
