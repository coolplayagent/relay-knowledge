use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    api::{
        CodeRepositoryFreshnessDiagnostics, CodeRepositoryFreshnessState, CodeRepositoryIndexLag,
        CodeRepositoryPendingIndexWork, CodeRepositoryRegisterRequest, InterfaceKind,
        RequestContext,
    },
    application::{RelayKnowledgeService, RuntimeConfiguration},
    code::{reset_tracked_entries_call_count_for_root, tracked_entries_call_count_for_root},
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeIndexResourceBudget,
        CodeIndexSession, CodeQueryKind, CodeRepositorySelector, CodeRetrievalHit,
        CodeRetrievalLayer, CodeRetrievalRequest, FreshnessPolicy, RepositoryCodeRange,
        StalenessHint,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{CodeRepositoryStore, SqliteGraphStore},
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
async fn allow_stale_query_reports_pending_freshness_and_source_read_requirement() {
    let repo = FixtureRepo::create("code-query-pending-freshness");
    repo.write("src/lib.rs", "pub fn pending_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-code-query-pending-freshness").await;
    let indexed = service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-code-query-pending-base"),
        )
        .await
        .expect("base index should succeed");
    repo.write("src/lib.rs", "pub fn pending_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "changed"]);
    let queued = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-code-query-pending-refresh"),
        )
        .await
        .expect("changed index should queue");

    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "pending_policy",
                selector("fixture", "HEAD"),
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-code-query-pending-freshness"),
        )
        .await
        .expect("allow-stale query should return previous index");

    assert!(query.metadata.stale);
    assert!(query.scope.stale);
    assert_eq!(query.results[0].path, "src/lib.rs");
    assert!(query.results[0].stale);
    assert_eq!(
        query.results[0].staleness_hint,
        Some(StalenessHint::PendingIndex {})
    );
    assert_eq!(query.freshness.state, CodeRepositoryFreshnessState::Pending);
    assert!(query.freshness.direct_source_read_required);
    assert_eq!(
        query.freshness.index_lag.served_ref,
        indexed.summary.resolved_commit_sha
    );
    assert_ne!(
        query.freshness.index_lag.requested_resolved_ref,
        query.freshness.index_lag.served_ref
    );
    assert!(!query.freshness.index_lag.requested_ref_indexed);
    assert!(query.freshness.pending.active_matches_request);
    assert_eq!(
        query.freshness.pending.active_task_id.as_deref(),
        queued.task.as_ref().map(|task| task.task_id.as_str())
    );
    assert_eq!(
        query.freshness.direct_source_read_paths,
        vec!["src/lib.rs".to_owned()]
    );
    assert!(
        query
            .freshness
            .agent_instructions
            .iter()
            .any(|instruction| instruction.contains("read direct source"))
    );
}

#[tokio::test]
async fn fresh_ref_query_ignores_unmatched_active_task_checkpoint() {
    let repo = FixtureRepo::create("code-query-unmatched-active-checkpoint");
    repo.write("src/lib.rs", "pub fn stable_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let initial_commit = repo.git_text(["rev-parse", "HEAD"]);
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(Arc::clone(&store)).await;

    register_fixture_repo(&service, &repo, "register-unmatched-active-checkpoint").await;
    let indexed = service
        .index_code_repository(
            request("fixture", &initial_commit),
            context("index-unmatched-active-base"),
        )
        .await
        .expect("initial commit should index");
    repo.write("src/lib.rs", "pub fn stable_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "changed"]);
    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("start-unmatched-active-head"),
        )
        .await
        .expect("head refresh should queue");
    let active_task = started.task.expect("active task should be queued");
    store
        .begin_code_index_session(CodeIndexSession {
            repository_id: active_task.repository_id.clone(),
            source_scope: active_task.source_scope.clone(),
            base_resolved_commit_sha: Some(initial_commit.clone()),
            resolved_commit_sha: active_task.resolved_commit_sha.clone(),
            tree_hash: active_task.tree_hash.clone(),
            path_filters: active_task.path_filters.clone(),
            language_filters: active_task.language_filters.clone(),
            full_replace: true,
            total_path_count: 8,
            changed_path_count: 8,
            skipped_unchanged_count: 0,
            deleted_paths: Vec::new(),
            tombstones: Vec::new(),
            resource_budget: CodeIndexResourceBudget::default(),
        })
        .await
        .expect("unmatched active checkpoint should begin");

    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "stable_policy",
                selector("fixture", &initial_commit),
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-unmatched-active-base"),
        )
        .await
        .expect("fresh indexed ref should query while another ref is active");

    assert_eq!(query.freshness.state, CodeRepositoryFreshnessState::Fresh);
    assert!(query.freshness.pending.active_for_repository);
    assert!(!query.freshness.pending.active_matches_request);
    assert_eq!(
        query
            .freshness
            .cursor
            .as_ref()
            .map(|cursor| cursor.source_scope.as_str()),
        Some(indexed.summary.source_scope.as_str())
    );
    assert_ne!(
        query
            .freshness
            .cursor
            .as_ref()
            .map(|cursor| cursor.source_scope.as_str()),
        Some(active_task.source_scope.as_str())
    );
}

#[test]
fn active_pending_match_keeps_fresh_hit_when_source_read_is_not_required() {
    let mut hits = vec![test_hit()];
    let freshness = freshness_with_active_match(false);

    super::repository::annotate_query_result_staleness(&mut hits, &freshness);

    assert!(!hits[0].stale);
    assert_eq!(hits[0].staleness_hint, Some(StalenessHint::Fresh));
}

fn freshness_with_active_match(
    direct_source_read_required: bool,
) -> CodeRepositoryFreshnessDiagnostics {
    CodeRepositoryFreshnessDiagnostics {
        state: if direct_source_read_required {
            CodeRepositoryFreshnessState::Pending
        } else {
            CodeRepositoryFreshnessState::Fresh
        },
        freshness_policy: FreshnessPolicy::AllowStale,
        graph_version: 1,
        source_scope: Some("scope".to_owned()),
        scope_stale: direct_source_read_required,
        stale_reason: None,
        degraded_reason: None,
        index_lag: CodeRepositoryIndexLag {
            requested_ref: "HEAD".to_owned(),
            requested_resolved_ref: "commit".to_owned(),
            served_ref: if direct_source_read_required {
                "previous".to_owned()
            } else {
                "commit".to_owned()
            },
            requested_ref_indexed: !direct_source_read_required,
            pending_file_count: None,
            pending_task_count: 1,
        },
        pending: CodeRepositoryPendingIndexWork {
            active_for_repository: true,
            active_matches_request: true,
            active_task_id: Some("task".to_owned()),
            active_task_state: Some("running".to_owned()),
            active_task_source_scope: Some("scope".to_owned()),
            active_task_ref_selector: Some("HEAD".to_owned()),
            active_task_resolved_commit_sha: Some("commit".to_owned()),
            active_task_lease_expires_at_ms: Some(2),
            queue_depth: 1,
            queued_task_count: 0,
            running_task_count: 1,
            retrying_task_count: 0,
            dead_letter_task_count: 0,
            running_lease_count: 1,
            last_error: None,
        },
        cursor: None,
        direct_source_read_required,
        direct_source_read_paths: Vec::new(),
        agent_instructions: Vec::new(),
    }
}

fn test_hit() -> CodeRetrievalHit {
    let range = RepositoryCodeRange { start: 1, end: 2 };
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        byte_range: range.clone(),
        line_range: range,
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: None,
        retrieval_layers: vec![CodeRetrievalLayer::Definition],
        index_versions: vec!["code:scope:tree".to_owned()],
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 1.0,
        excerpt: "pub fn stable_policy() -> u32 { 1 }".to_owned(),
    }
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
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    service_with_store(store).await
}

async fn service_with_store(store: Arc<SqliteGraphStore>) -> RelayKnowledgeService {
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
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
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
