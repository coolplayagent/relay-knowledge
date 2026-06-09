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
        CodeIndexMode, CodeIndexRequest, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, CodeRepositorySetAddMemberRequest,
        CodeRepositorySetCreateRequest, CodeRepositorySetQueryRequest, CodeRetrievalLayer,
        FreshnessPolicy, RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        code_snapshot_scope_id,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeRepositorySetMemberSeed, CodeRepositorySetSeed, CodeRepositoryStore, KnowledgeStore,
        SqliteGraphStore,
    },
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
async fn repository_set_member_reuses_broad_non_git_scope_for_narrow_filters() {
    let source = PlainSourceDir::create("repo-set-non-git-broad-member");
    source.write(
        "src/api/public.rs",
        r#"
pub fn shared_target() -> u32 {
    1
}
"#,
    );
    source.write(
        "src/internal/hidden.rs",
        r#"
pub fn hidden_target() -> u32 {
    2
}
"#,
    );
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: source.path.display().to_string(),
                alias: "plain".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-non-git-broad-member"),
        )
        .await
        .expect("non-git repository should register");
    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("plain"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-non-git-broad-member"),
        )
        .await
        .expect("broad non-git repository should index");
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new("workspace", None, None)
                .expect("set request should validate"),
            context("create-non-git-broad-member-set"),
        )
        .await
        .expect("set should create");

    let added = service
        .add_code_repository_set_member(
            CodeRepositorySetAddMemberRequest::new(
                "workspace",
                "plain",
                "HEAD",
                vec!["src/api".to_owned()],
                Vec::new(),
                0,
            )
            .expect("member request should validate"),
            context("add-non-git-broad-member"),
        )
        .await
        .expect("narrow member should reuse broad non-git scope");

    assert_eq!(
        added.member.resolved_commit_sha,
        indexed.summary.resolved_commit_sha
    );
    assert_eq!(added.member.source_scope, indexed.summary.source_scope);

    let status = service
        .code_repository_set_status(
            "workspace".to_owned(),
            context("status-non-git-broad-member"),
        )
        .await
        .expect("set status should keep compatible broad member fresh");

    assert_eq!(status.status.members[0].freshness_state, "fresh");
    assert!(!status.status.members[0].stale);
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
async fn repository_set_refresh_persists_fact_version_member_scope_before_overlay() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    let current_scope = code_snapshot_scope_id("repo-app", "tree-current", &[], &[]);
    let legacy_scope = "git_snapshot:0000000000000000";

    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo-app", "app", "/tmp/app", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot_for_scope(
            "repo-app",
            &current_scope,
            "tree-current",
        ))
        .await
        .expect("current scope should persist");
    store
        .apply_code_index_snapshot(snapshot_for_scope("repo-app", legacy_scope, "tree-current"))
        .await
        .expect("legacy scope should persist");
    store
        .create_code_repository_set(CodeRepositorySetSeed {
            alias: "workspace".to_owned(),
            description: None,
            default_ref_policy_json: "{}".to_owned(),
            now_ms: 1,
        })
        .await
        .expect("set should persist");
    store
        .add_code_repository_set_member(CodeRepositorySetMemberSeed {
            set_alias: "workspace".to_owned(),
            repository_id: "repo-app".to_owned(),
            repository_alias: "app".to_owned(),
            ref_selector: "commit".to_owned(),
            resolved_commit_sha: "commit".to_owned(),
            source_scope: legacy_scope.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            priority: 0,
        })
        .await
        .expect("legacy member should persist");

    let response = service
        .refresh_code_repository_set("workspace".to_owned(), context("fact-refresh"))
        .await
        .expect("set refresh should persist the replacement scope first");
    let persisted = store
        .code_repository_set_status("workspace".to_owned())
        .await
        .expect("set status should load")
        .expect("set should exist");

    assert_eq!(
        response.status.members[0].member.source_scope,
        current_scope
    );
    assert_eq!(persisted.members[0].member.source_scope, current_scope);
    assert_eq!(persisted.overlay.state, "fresh");
    assert!(!persisted.overlay.stale);
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
                workspace_detection: Default::default(),
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
                language_filters: Vec::new(),
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
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    service_with_store(store).await
}

async fn service_with_store(store: Arc<dyn KnowledgeStore>) -> RelayKnowledgeService {
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

    RelayKnowledgeService::with_store(runtime, store)
}

fn snapshot_for_scope(
    repository_id: &str,
    source_scope: &str,
    tree_hash: &str,
) -> CodeIndexSnapshot {
    let path = "src/lib.rs";
    let content = "pub fn current() {}\n";
    let byte_end = u32::try_from(content.len()).expect("fixture content should fit in u32");
    CodeIndexSnapshot {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![RepositoryCodeFileRecord {
            repository_id: repository_id.to_owned(),
            source_scope: source_scope.to_owned(),
            file_id: format!("file-{source_scope}"),
            path: path.to_owned(),
            language_id: "rust".to_owned(),
            blob_hash: format!("blob-{source_scope}"),
            byte_len: content.len(),
            line_count: 1,
            parse_status: CodeParseStatus::Parsed,
            is_generated: false,
            degraded_reason: None,
        }],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![RepositoryCodeChunkRecord {
            repository_id: repository_id.to_owned(),
            source_scope: source_scope.to_owned(),
            chunk_id: format!("chunk-{source_scope}"),
            file_id: format!("file-{source_scope}"),
            path: path.to_owned(),
            language_id: "rust".to_owned(),
            content: content.to_owned(),
            byte_range: RepositoryCodeRange {
                start: 0,
                end: byte_end,
            },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
            symbol_snapshot_id: None,
        }],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

struct FixtureRepo {
    path: PathBuf,
}

struct PlainSourceDir {
    path: PathBuf,
}

impl PlainSourceDir {
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
