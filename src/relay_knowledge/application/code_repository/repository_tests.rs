use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    code::{reset_tracked_entries_call_count_for_root, tracked_entries_call_count_for_root},
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

static TRACKED_ENTRIES_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn git_fresh_full_index_skips_tracked_entry_plan_build() {
    let _guard = TRACKED_ENTRIES_TEST_LOCK.lock().await;
    let repo = FixtureRepo::create("git-full-noop-fast-path");
    repo.write("src/lib.rs", "pub fn stable_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    let observed_root = repo
        .path
        .canonicalize()
        .expect("repo path should canonicalize");

    register_fixture_repo(&service, &repo, "register-git-full-noop-fast-path").await;
    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let first = service
        .index_code_repository(request("fixture", "HEAD"), context("index-git-full-first"))
        .await
        .expect("initial full index should succeed");
    assert!(
        tracked_entries_call_count_for_root(&observed_root) > 0,
        "cold full index should enumerate tracked entries"
    );

    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let second = service
        .index_code_repository(request("fixture", "HEAD"), context("index-git-full-second"))
        .await
        .expect("fresh full index should reuse scope");

    assert_eq!(second.summary.source_scope, first.summary.source_scope);
    assert_eq!(second.summary.progress.blob_read_count, 0);
    assert_eq!(tracked_entries_call_count_for_root(&observed_root), 0);
}

#[tokio::test]
async fn duplicate_active_full_index_start_skips_tracked_entry_plan_build() {
    let _guard = TRACKED_ENTRIES_TEST_LOCK.lock().await;
    let repo = FixtureRepo::create("git-active-duplicate-fast-path");
    repo.write("src/lib.rs", "pub fn queued_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;
    let observed_root = repo
        .path
        .canonicalize()
        .expect("repo path should canonicalize");
    let request = CodeIndexRequest {
        repository: selector("fixture", "HEAD"),
        mode: CodeIndexMode::Full,
        freshness_policy: FreshnessPolicy::AllowStale,
    };

    register_fixture_repo(&service, &repo, "register-git-active-duplicate-fast-path").await;
    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let first = service
        .start_code_repository_index(request.clone(), context("start-git-active-first"))
        .await
        .expect("cold full index should queue");

    reset_tracked_entries_call_count_for_root(observed_root.clone());
    let duplicate = service
        .start_code_repository_index(request, context("start-git-active-duplicate"))
        .await
        .expect("duplicate full index should reuse queued task");

    assert_eq!(
        duplicate.task.as_ref().map(|task| task.task_id.as_str()),
        first.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert_eq!(tracked_entries_call_count_for_root(&observed_root), 0);

    let distinct_request = CodeIndexRequest {
        repository: CodeRepositorySelector::new(
            "fixture",
            "HEAD",
            vec!["src".to_owned()],
            Vec::new(),
        )
        .expect("selector should validate"),
        mode: CodeIndexMode::Full,
        freshness_policy: FreshnessPolicy::AllowStale,
    };
    reset_tracked_entries_call_count_for_root(observed_root.clone());
    service
        .start_code_repository_index(distinct_request, context("start-git-active-distinct"))
        .await
        .expect("distinct full index request should build a plan");
    assert!(
        tracked_entries_call_count_for_root(&observed_root) > 0,
        "non-identical active full-index starts should still build a plan"
    );
}

#[tokio::test]
async fn duplicate_active_filesystem_full_index_resolves_live_snapshot_before_reuse() {
    let source = FixtureSourceDir::create("filesystem-active-duplicate-current-ref");
    source.write("src/lib.rs", "pub fn queued_policy() -> u32 { 1 }\n");
    let service = service_with_memory_store().await;
    let request = CodeIndexRequest {
        repository: selector("fixture", "HEAD"),
        mode: CodeIndexMode::Full,
        freshness_policy: FreshnessPolicy::AllowStale,
    };

    register_fixture_source(&service, &source, "register-filesystem-active-current-ref").await;
    let first = service
        .start_code_repository_index(request.clone(), context("start-filesystem-active-first"))
        .await
        .expect("cold filesystem full index should queue");
    source.write("src/lib.rs", "pub fn queued_policy() -> u32 { 2 }\n");
    let changed = service
        .start_code_repository_index(request, context("start-filesystem-active-changed"))
        .await
        .expect("changed filesystem full index should queue new snapshot");

    assert_ne!(
        changed.task.as_ref().map(|task| task.task_id.as_str()),
        first.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert_ne!(
        changed
            .task
            .as_ref()
            .map(|task| task.resolved_commit_sha.as_str()),
        first
            .task
            .as_ref()
            .map(|task| task.resolved_commit_sha.as_str())
    );
}

#[tokio::test]
async fn filesystem_impact_reports_deleted_base_paths_from_stored_fingerprints() {
    let source = FixtureSourceDir::create("filesystem-impact-deleted-base-path");
    source.write("src/lib.rs", "pub fn unchanged_policy() -> u32 { 1 }\n");
    source.write("src/api.rs", "pub fn removed_policy() -> u32 { 1 }\n");
    let service = service_with_memory_store().await;

    register_fixture_source(&service, &source, "register-filesystem-impact-delete").await;
    let base = service
        .index_code_repository(request("fixture", "HEAD"), context("index-filesystem-base"))
        .await
        .expect("base filesystem index should succeed");
    fs::remove_file(source.path.join("src/api.rs")).expect("fixture source should delete");
    service
        .index_code_repository(request("fixture", "HEAD"), context("index-filesystem-head"))
        .await
        .expect("head filesystem index should succeed");

    let impact = service
        .impact_code_repository(
            CodeImpactRequest::new(
                selector("fixture", "HEAD"),
                base.summary.resolved_commit_sha,
                "HEAD",
                10,
            )
            .expect("impact request should validate"),
            context("impact-filesystem-delete"),
        )
        .await
        .expect("filesystem impact should succeed");

    assert_eq!(
        impact.path_groups.in_scope_changed_paths,
        ["src/api.rs".to_owned()]
    );
}

async fn register_fixture_repo(service: &RelayKnowledgeService, repo: &FixtureRepo, name: &str) {
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

async fn register_fixture_source(
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
        .expect("source directory should register");
}

fn request(alias: &str, ref_selector: &str) -> CodeIndexRequest {
    CodeIndexRequest {
        repository: selector(alias, ref_selector),
        mode: CodeIndexMode::Full,
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
    }
}

fn selector(alias: &str, ref_selector: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, Vec::new(), Vec::new())
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

struct FixtureSourceDir {
    path: PathBuf,
}

impl FixtureSourceDir {
    fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-{name}-{nanos}"));
        fs::create_dir_all(path.join("src")).expect("source directory should be created");
        Self { path }
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory should exist");
        }
        fs::write(path, content).expect("fixture file should be written");
    }
}
