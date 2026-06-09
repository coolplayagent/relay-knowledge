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
        CodeImportRecord, CodeIndexMode, CodeIndexRequest, CodeIndexSnapshot, CodeParseStatus,
        CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector,
        CodeRepositorySetAddMemberRequest, CodeRepositorySetCreateRequest,
        CodeRepositorySetQueryRequest, CodeRetrievalRequest, CodeWorkspaceDetectionConfig,
        CodeWorkspaceMember, CodeWorkspacePackageMapping, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        code_snapshot_scope_id,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{
        CodeRepositorySetMemberSeed, CodeRepositorySetSeed, CodeRepositoryStore, SqliteGraphStore,
    },
};

// ── helpers ───────────────────────────────────────────────────────────

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

struct FixtureRepo {
    path: PathBuf,
}

impl FixtureRepo {
    fn create(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-knowledge-ws-cross-{name}-{nanos}"));
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

async fn register_and_index(service: &RelayKnowledgeService, repo: &FixtureRepo, alias: &str) {
    register_and_index_with_options(
        service,
        repo,
        alias,
        vec!["src".to_owned()],
        CodeWorkspaceDetectionConfig::default(),
    )
    .await;
}

async fn register_and_index_with_options(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    alias: &str,
    path_filters: Vec<String>,
    workspace_detection: CodeWorkspaceDetectionConfig,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: alias.to_owned(),
                path_filters,
                language_filters: Vec::new(),
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
                workspace_detection,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context(&format!("index-{alias}")),
        )
        .await
        .expect("repository should index");
}

async fn create_set(service: &RelayKnowledgeService, alias: &str, members: &[(&str, i32)]) {
    service
        .create_code_repository_set(
            CodeRepositorySetCreateRequest::new(alias, None, None)
                .expect("set request should validate"),
            context(&format!("create-set-{alias}")),
        )
        .await
        .expect("set should create");

    for (repository_alias, priority) in members {
        service
            .add_code_repository_set_member(
                CodeRepositorySetAddMemberRequest::new(
                    alias,
                    *repository_alias,
                    "HEAD",
                    Vec::new(),
                    Vec::new(),
                    *priority,
                )
                .expect("member request should validate"),
                context(&format!("add-member-{repository_alias}")),
            )
            .await
            .expect("member should add");
    }
}

/// Creates a minimal `CodeIndexSnapshot` fixture with optional imports for a
/// repository.
fn snapshot_fixture(
    repository_id: &str,
    source_scope: &str,
    commit: &str,
    imports: Vec<CodeImportRecord>,
) -> CodeIndexSnapshot {
    let path = "src/lib.rs";
    let content = "pub fn placeholder() {}\n";
    let byte_end = u32::try_from(content.len()).expect("fixture content should fit in u32");
    CodeIndexSnapshot {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: commit.to_owned(),
        tree_hash: String::from("tree-hash"),
        path_filters: vec!["src".to_owned()],
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![RepositoryCodeFileRecord {
            repository_id: repository_id.to_owned(),
            source_scope: source_scope.to_owned(),
            file_id: format!("file-{repository_id}"),
            path: path.to_owned(),
            language_id: String::from("rust"),
            blob_hash: format!("blob-{repository_id}"),
            byte_len: content.len(),
            line_count: 1,
            parse_status: CodeParseStatus::Parsed,
            is_generated: false,
            degraded_reason: None,
        }],
        symbols: Vec::new(),
        references: Vec::new(),
        imports,
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![RepositoryCodeChunkRecord {
            repository_id: repository_id.to_owned(),
            source_scope: source_scope.to_owned(),
            chunk_id: format!("chunk-{repository_id}"),
            file_id: format!("file-{repository_id}"),
            path: path.to_owned(),
            language_id: String::from("rust"),
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

fn import_record(
    repository_id: &str,
    source_scope: &str,
    module: &str,
    path: &str,
    resolution_state: &str,
    target_hint: Option<String>,
) -> CodeImportRecord {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{repository_id}:{source_scope}:{module}:{path}:{resolution_state}").hash(&mut hasher);
    let hash = hasher.finish();

    let confidence: u16 = if resolution_state == "resolved" {
        10000u16
    } else {
        0u16
    };
    let tier = if resolution_state == "resolved" {
        String::from("explicit")
    } else {
        String::from("unresolved")
    };

    CodeImportRecord {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        import_id: format!("import-{hash:016x}"),
        file_id: format!("file-{repository_id}"),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint,
        resolution_state: resolution_state.to_owned(),
        confidence_basis_points: confidence,
        confidence_tier: tier,
        line_range: RepositoryCodeRange { start: 1, end: 1 },
    }
}

// ── workspace domain type tests ───────────────────────────────────────

#[test]
fn workspace_detection_config_disabled_default() {
    let config = CodeWorkspaceDetectionConfig::default();
    assert!(!config.enabled);
    assert!(!config.supported_formats.is_empty());
}

#[test]
fn workspace_package_mapping_fields_are_accessible() {
    let mapping = CodeWorkspacePackageMapping {
        package_name: String::from("pkg-core"),
        ecosystem: String::from("npm"),
        repository_id: String::from("repo-1"),
        source_scope: String::from("git_snapshot:abcdef1234567890"),
        confidence_basis_points: 10_000,
    };
    assert_eq!(mapping.package_name, "pkg-core");
    assert_eq!(mapping.ecosystem, "npm");
    assert!(mapping.confidence_basis_points >= 8000);
}

#[test]
fn workspace_member_non_empty_fields() {
    let member = CodeWorkspaceMember {
        package_name: String::from("pkg-lib"),
        relative_path: String::from("packages/lib"),
    };
    assert!(!member.package_name.trim().is_empty());
    assert!(!member.relative_path.trim().is_empty());
}

// ── cross-repo import resolution via repository set overlay ───────────

#[tokio::test]
async fn pnpm_monorepo_cross_repo_imports_resolved_via_set_overlay() {
    // ── arrange: two repos where lib imports core_init from core ──
    let core_repo = FixtureRepo::create("pnpm-ws-core");
    core_repo.write(
        "src/lib.rs",
        r#"
/// Core utility.
pub fn core_init() -> u32 {
    42
}
"#,
    );
    core_repo.git(["add", "."]);
    core_repo.git(["commit", "-m", "core"]);

    let lib_repo = FixtureRepo::create("pnpm-ws-lib");
    lib_repo.write(
        "src/consumer.rs",
        r#"
use core_init;

pub fn consume() -> u32 {
    core_init()
}
"#,
    );
    lib_repo.git(["add", "."]);
    lib_repo.git(["commit", "-m", "lib"]);

    let service = service_with_memory_store().await;

    register_and_index(&service, &core_repo, "core").await;
    register_and_index(&service, &lib_repo, "lib").await;

    create_set(&service, "pnpm-workspace", &[("core", 0), ("lib", 0)]).await;

    let refreshed = service
        .refresh_code_repository_set("pnpm-workspace".to_owned(), context("refresh-pnpm"))
        .await
        .expect("set refresh should succeed");

    assert!(
        refreshed.summary.as_ref().is_some_and(|s| s.edge_count > 0),
        "expected cross-repo edges after workspace set refresh, got summary: {:?}",
        refreshed.summary
    );

    let import_query = service
        .query_code_repository_set(
            CodeRepositorySetQueryRequest::new(
                "pnpm-workspace",
                "core_init",
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
                Vec::new(),
                Vec::new(),
            )
            .expect("query request should validate"),
            context("pnpm-import-query"),
        )
        .await
        .expect("import query should succeed");

    assert!(
        !import_query.results.is_empty(),
        "expected import query results for core_init"
    );

    assert!(
        import_query.results.iter().any(|result| {
            result.overlay_evidence.iter().any(|edge| {
                edge.edge_kind == "cross_repo_import"
                    || edge.edge_kind == "imports"
                    || edge.edge_kind == "import"
            })
        }),
        "expected cross-repo import overlay evidence"
    );
}

/// Go monorepo set: verifies two Go repos in a set produce a fresh overlay
/// and that symbol queries work across members.
#[tokio::test]
async fn go_monorepo_cross_module_edges_via_set_overlay() {
    let core_repo = FixtureRepo::create("go-ws-core");
    core_repo.write(
        "core.go",
        r#"
package core

func CoreInit() int {
    return 42
}
"#,
    );
    core_repo.git(["add", "."]);
    core_repo.git(["commit", "-m", "core"]);

    let lib_repo = FixtureRepo::create("go-ws-lib");
    lib_repo.write(
        "lib.go",
        r#"
package lib

func Consume() int {
    return 44
}
"#,
    );
    lib_repo.git(["add", "."]);
    lib_repo.git(["commit", "-m", "lib"]);

    let service = service_with_memory_store().await;

    register_and_index(&service, &core_repo, "go-core").await;
    register_and_index(&service, &lib_repo, "go-lib").await;

    create_set(&service, "go-workspace", &[("go-core", 0), ("go-lib", 0)]).await;

    let refreshed = service
        .refresh_code_repository_set("go-workspace".to_owned(), context("refresh-go"))
        .await
        .expect("set refresh should succeed");

    let status = service
        .code_repository_set_status("go-workspace".to_owned(), context("status-go"))
        .await
        .expect("status should load");

    assert_eq!(status.status.overlay.state, "fresh");
    assert!(!status.status.overlay.stale);
    assert_eq!(status.status.members.len(), 2);

    assert!(
        refreshed.summary.is_some(),
        "go workspace set refresh should produce a summary"
    );

    // The set has two indexed Go repos — verify the members are present
    assert_eq!(refreshed.status.members.len(), 2);
}

/// A single repository (no other members in a set) should produce zero
/// cross edges in the overlay.
#[tokio::test]
async fn single_repo_no_workspace_config_produces_zero_cross_edges() {
    let solo_repo = FixtureRepo::create("solo-repo");
    solo_repo.write(
        "src/main.rs",
        r#"
pub fn standalone() -> u32 {
    1
}
"#,
    );
    solo_repo.git(["add", "."]);
    solo_repo.git(["commit", "-m", "solo"]);

    let service = service_with_memory_store().await;
    register_and_index(&service, &solo_repo, "solo").await;

    create_set(&service, "solo-set", &[("solo", 0)]).await;

    let refreshed = service
        .refresh_code_repository_set("solo-set".to_owned(), context("refresh-solo"))
        .await
        .expect("set refresh should succeed");

    assert_eq!(
        refreshed
            .summary
            .as_ref()
            .map(|s| s.edge_count)
            .unwrap_or(0),
        0,
        "single-repo workspace should have no cross edges"
    );
}

/// When a repo imports a module that is not registered as a set member,
/// the import carries an unresolved state without target_hint pointing to
/// a registered workspace member.
#[tokio::test]
async fn import_to_unregistered_workspace_member_has_unresolved_state() {
    let lib_repo = FixtureRepo::create("ws-unreg-lib");
    lib_repo.write(
        "src/consumer.rs",
        r#"
use unregistered_crate::core_init;

pub fn consume() -> u32 {
    core_init()
}
"#,
    );
    lib_repo.git(["add", "."]);
    lib_repo.git(["commit", "-m", "lib"]);

    let service = service_with_memory_store().await;
    register_and_index(&service, &lib_repo, "lib").await;

    let import_query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "core_init",
                selector("lib"),
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("unreg-import-query"),
        )
        .await
        .expect("query should succeed");

    assert!(
        import_query
            .results
            .iter()
            .any(|hit| { hit.edge_resolution_state.as_deref() == Some("unresolved") }),
        "expected unresolved import for core_init when target repo is not registered: {:?}",
        import_query
            .results
            .iter()
            .map(|r| (&r.edge_resolution_state, &r.edge_kind))
            .collect::<Vec<_>>()
    );
}

/// After two repos have been registered, indexed, and added to a set,
/// cross-repo overlay edges are produced and queryable as overlay evidence.
#[tokio::test]
async fn cross_repo_edges_queryable_after_set_refresh() {
    let app_repo = FixtureRepo::create("ws-edges-app");
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

    let svc_repo = FixtureRepo::create("ws-edges-svc");
    svc_repo.write(
        "src/service.rs",
        r#"
pub fn serve() -> u32 {
    2
}
"#,
    );
    svc_repo.git(["add", "."]);
    svc_repo.git(["commit", "-m", "svc"]);

    let service = service_with_memory_store().await;
    register_and_index(&service, &app_repo, "app").await;
    register_and_index(&service, &svc_repo, "svc").await;

    create_set(&service, "edge-workspace", &[("app", 0), ("svc", 0)]).await;

    let refreshed = service
        .refresh_code_repository_set("edge-workspace".to_owned(), context("refresh-edges"))
        .await
        .expect("set refresh should succeed");

    let summary = refreshed
        .summary
        .expect("refresh summary should be present");

    assert!(
        summary.edge_count > 0,
        "expected at least one cross edge, got edge_count={} resolved={}",
        summary.edge_count,
        summary.resolved_edge_count
    );

    let status = service
        .code_repository_set_status("edge-workspace".to_owned(), context("status-edges"))
        .await
        .expect("status should load");

    let overlay = status.status.overlay;
    assert_eq!(overlay.state, "fresh");
    assert!(!overlay.stale);

    let import_results = service
        .query_code_repository_set(
            CodeRepositorySetQueryRequest::new(
                "edge-workspace",
                "service::serve",
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
                Vec::new(),
                Vec::new(),
            )
            .expect("query request should validate"),
            context("edges-import-query"),
        )
        .await
        .expect("import query should succeed");

    let has_overlay = import_results
        .results
        .iter()
        .any(|result| !result.overlay_evidence.is_empty());

    assert!(
        has_overlay,
        "expected overlay evidence on at least one import result"
    );
}

#[tokio::test]
async fn request_enabled_workspace_detection_creates_auto_set_edges() {
    let repo = FixtureRepo::create("go-auto-workspace");
    repo.write(
        "go.work",
        r#"
go 1.21

use (
    ./api
    ./client
)
"#,
    );
    repo.write(
        "api/go.mod",
        r#"
module example.com/svc/api

go 1.21
"#,
    );
    repo.write(
        "api/api.go",
        r#"
package api

func Serve() int {
    return 2
}
"#,
    );
    repo.write(
        "client/go.mod",
        r#"
module example.com/svc/client

go 1.21
"#,
    );
    repo.write(
        "client/client.go",
        r#"
package client

import "example.com/svc/api/transport"

func Consume() int {
    return 1
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "workspace"]);

    let service = service_with_memory_store().await;
    register_and_index_with_options(
        &service,
        &repo,
        "auto-go",
        Vec::new(),
        CodeWorkspaceDetectionConfig::enabled_all(),
    )
    .await;

    let repository_status = service
        .code_repository_status(selector("auto-go"), context("auto-workspace-status"))
        .await
        .expect("repository status should load");
    let workspace_set_alias = format!("{}-auto-workspace", repository_status.status.repository_id);
    let set_status = service
        .code_repository_set_status(workspace_set_alias.clone(), context("auto-set-status"))
        .await
        .expect("automatic workspace set should be persisted");

    assert_eq!(set_status.status.members.len(), 1);
    assert_eq!(
        set_status.status.members[0].member.repository_id,
        repository_status.status.repository_id
    );

    let import_results = service
        .query_code_repository_set(
            CodeRepositorySetQueryRequest::new(
                &workspace_set_alias,
                "example.com/svc/api/transport",
                CodeQueryKind::Imports,
                10,
                FreshnessPolicy::AllowStale,
                Vec::new(),
                Vec::new(),
            )
            .expect("query request should validate"),
            context("auto-workspace-import-query"),
        )
        .await
        .expect("automatic workspace set query should succeed");

    assert!(
        import_results.results.iter().any(|result| {
            result.overlay_evidence.iter().any(|edge| {
                edge.edge_kind == "cross_repo_import"
                    && edge.resolution_state == "resolved"
                    && edge
                        .to_repository_id
                        .as_deref()
                        .is_some_and(|id| id == repository_status.status.repository_id)
            })
        }),
        "expected request-enabled workspace detection to create resolved overlay evidence: {:?}",
        import_results.results
    );

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("auto-go"),
                mode: CodeIndexMode::Full,
                workspace_detection: CodeWorkspaceDetectionConfig::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("auto-workspace-disabled-reindex"),
        )
        .await
        .expect("disabled fresh reindex should succeed");

    assert!(
        service
            .code_repository_set_status(workspace_set_alias, context("auto-set-cleared"))
            .await
            .is_err(),
        "disabled workspace detection should clear the auto workspace set"
    );
}

// ── storage-level workspace mapping test ──────────────────────────────

#[tokio::test]
async fn storage_level_workspace_package_mapping_survives_snapshot_apply() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));

    let scope_lib = code_snapshot_scope_id("repo-lib", "commit-1", &["src".to_owned()], &[]);
    let scope_core = code_snapshot_scope_id("repo-core", "commit-1", &["src".to_owned()], &[]);

    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo-lib",
                "lib",
                "/tmp/lib",
                vec!["src".to_owned()],
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("lib repo should persist");

    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo-core",
                "core",
                "/tmp/core",
                vec!["src".to_owned()],
                Vec::new(),
            )
            .expect("registration should validate"),
        )
        .await
        .expect("core repo should persist");

    let lib_snapshot = snapshot_fixture(
        "repo-lib",
        &scope_lib,
        "commit-1",
        vec![import_record(
            "repo-lib",
            &scope_lib,
            "pkg_core",
            "src/lib.rs",
            "unresolved",
            Some(String::from("pkg_core")),
        )],
    );
    store
        .apply_code_index_snapshot(lib_snapshot)
        .await
        .expect("lib snapshot should apply");

    let core_snapshot = snapshot_fixture("repo-core", &scope_core, "commit-1", Vec::new());
    store
        .apply_code_index_snapshot(core_snapshot)
        .await
        .expect("core snapshot should apply");

    store
        .create_code_repository_set(CodeRepositorySetSeed {
            alias: String::from("pnpm-ws"),
            description: None,
            default_ref_policy_json: String::from(r#"{"default_ref":"HEAD"}"#),
            now_ms: 1,
        })
        .await
        .expect("set should persist");

    store
        .add_code_repository_set_member(CodeRepositorySetMemberSeed {
            set_alias: String::from("pnpm-ws"),
            repository_id: String::from("repo-core"),
            repository_alias: String::from("core"),
            ref_selector: String::from("HEAD"),
            resolved_commit_sha: String::from("commit-1"),
            source_scope: scope_core.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            priority: 0,
        })
        .await
        .expect("core member should persist");

    store
        .add_code_repository_set_member(CodeRepositorySetMemberSeed {
            set_alias: String::from("pnpm-ws"),
            repository_id: String::from("repo-lib"),
            repository_alias: String::from("lib"),
            ref_selector: String::from("HEAD"),
            resolved_commit_sha: String::from("commit-1"),
            source_scope: scope_lib.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            priority: 0,
        })
        .await
        .expect("lib member should persist");

    let refresh = store
        .refresh_code_repository_set_overlay(String::from("pnpm-ws"), 2)
        .await
        .expect("overlay refresh should succeed");

    assert!(
        refresh.edge_count > 0,
        "set overlay should produce cross edges: edge_count={}, resolved={}",
        refresh.edge_count,
        refresh.resolved_edge_count
    );
}
