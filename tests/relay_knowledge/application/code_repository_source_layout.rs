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
        CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn default_src_registration_indexes_discovered_external_source_roots() {
    let repo = FixtureRepo::create("code-source-layout");
    repo.write("src/lib.rs", "pub fn local_entry() {}\n");
    repo.write(
        "external_deps/rust_sdk/lib.rs",
        "pub fn external_session_client() {}\n",
    );
    repo.write("tests/helper.rs", "pub fn test_helper() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register"),
        )
        .await
        .expect("repository should register");
    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD", Vec::new()),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index"),
        )
        .await
        .expect("repository should index");

    assert_eq!(indexed.summary.indexed_file_count, 2);

    let external = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "external_session_client",
                selector("fixture", "HEAD", Vec::new()),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query should validate"),
            context("query"),
        )
        .await
        .expect("external source query should succeed");
    let external_filtered = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "external_session_client",
                selector("fixture", "HEAD", vec!["external_deps/rust_sdk".to_owned()]),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query should validate"),
            context("query-filtered"),
        )
        .await
        .expect("external filtered query should succeed");
    let tests_filtered = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "test_helper",
                selector("fixture", "HEAD", vec!["tests".to_owned()]),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query should validate"),
            context("query-tests"),
        )
        .await;

    assert!(external.results.iter().any(|hit| {
        hit.path == "external_deps/rust_sdk/lib.rs"
            && hit.excerpt.contains("external_session_client")
    }));
    assert!(
        external
            .scope
            .path_filters
            .contains(&"external_deps/rust_sdk".to_owned())
    );
    assert!(
        external_filtered
            .results
            .iter()
            .any(|hit| hit.path == "external_deps/rust_sdk/lib.rs")
    );
    assert!(tests_filtered.is_err());
}

#[tokio::test]
async fn historical_reuse_matches_discovered_effective_source_roots() {
    let repo = FixtureRepo::create("code-source-layout-history");
    repo.write("src/lib.rs", "pub fn local_entry() -> u32 { 1 }\n");
    repo.write(
        "external_deps/rust_sdk/lib.rs",
        "pub fn external_session_client() {}\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-history"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD", Vec::new()),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-history-base"),
        )
        .await
        .expect("base should index with its discovered source root");

    repo.write("src/lib.rs", "pub fn local_entry() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "head"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);
    let request = CodeIndexRequest {
        repository: selector("fixture", "HEAD", Vec::new()),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        reuse_historical: true,
    };
    let started = service
        .start_code_repository_index(request.clone(), context("start-history-head"))
        .await
        .expect("historical reuse should accept effective discovered filters");
    let task = started.task.expect("new head should queue a task");
    assert_eq!(
        task.mode,
        CodeIndexMode::incremental(base.clone(), head)
            .expect("resolved commits should form an incremental mode")
    );
    assert!(
        task.path_filters
            .contains(&"external_deps/rust_sdk".to_owned())
    );
    let duplicate = service
        .start_code_repository_index(request.clone(), context("repeat-history-head"))
        .await
        .expect("same effective discovered scope should reuse its task");
    assert_eq!(
        duplicate.task.as_ref().map(|queued| &queued.task_id),
        Some(&task.task_id)
    );

    let completed = service
        .index_code_repository(request, context("index-history-head"))
        .await
        .expect("historical incremental task should complete");
    assert_eq!(
        completed.summary.base_resolved_commit_sha.as_deref(),
        Some(base.as_str())
    );
    assert_eq!(completed.summary.progress.parsed_file_count, 1);
}

fn selector(alias: &str, ref_selector: &str, path_filters: Vec<String>) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, path_filters, Vec::new())
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
    let environment = test_environment();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    RelayKnowledgeService::with_store(runtime, store)
}

#[cfg(windows)]
fn test_environment() -> EnvironmentConfig {
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
fn test_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse")
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
            .expect("git output should be UTF-8")
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
