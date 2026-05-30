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
        CodeRetrievalRequest, FreshnessPolicy, SoftwareGlobalKind, SoftwareGlobalRequest,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn software_projection_resolves_symbolic_refs_to_indexed_commit_scope() {
    let repo = FixtureRepo::create("code-software-ref");
    repo.write(
        "Cargo.toml",
        r#"
[package]
name = "fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
"#,
    );
    repo.write("src/lib.rs", "pub fn uses_dependency_manifest() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "software manifest"]);
    repo.git(["branch", "software-main"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, Vec::new(), "register-software-ref").await;

    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "software-main"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-software-ref"),
        )
        .await
        .expect("symbolic ref should index");
    let projection =
        software_projection(&service, "software-main", FreshnessPolicy::WaitUntilFresh)
            .await
            .expect("symbolic software ref should resolve to indexed commit");

    assert_eq!(projection.scope.requested_ref, "software-main");
    assert_eq!(projection.scope.scope_id, indexed.scope.scope_id);
    assert!(
        projection
            .components
            .iter()
            .any(|component| component.name == "serde")
    );
}

#[tokio::test]
async fn software_projection_metadata_reports_selected_old_scope() {
    let repo = FixtureRepo::create("code-software-old-scope");
    repo.write(
        "Cargo.toml",
        r#"
[package]
name = "fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
"#,
    );
    repo.write("src/lib.rs", "pub fn old_dependency_manifest() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "old software manifest"]);
    let old_commit = repo.git_text(["rev-parse", "HEAD"]);
    repo.write(
        "Cargo.toml",
        r#"
[package]
name = "fixture"
version = "0.2.0"
edition = "2021"

[dependencies]
tokio = "1"
"#,
    );
    repo.write("src/lib.rs", "pub fn new_dependency_manifest() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "new software manifest"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, Vec::new(), "register-software-old").await;

    let old_index = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", &old_commit),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-software-old"),
        )
        .await
        .expect("old commit should index");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-software-head"),
        )
        .await
        .expect("head should index");

    let projection = software_projection(&service, &old_commit, FreshnessPolicy::WaitUntilFresh)
        .await
        .expect("old software scope should load");

    assert_eq!(projection.scope.scope_id, old_index.scope.scope_id);
    assert_eq!(projection.scope.resolved_commit_sha, old_commit);
    assert!(
        projection
            .components
            .iter()
            .any(|component| component.name == "serde")
    );
    assert!(
        !projection
            .components
            .iter()
            .any(|component| component.name == "tokio")
    );
}

#[tokio::test]
async fn software_projection_accepts_canonical_path_filter_spellings() {
    let repo = FixtureRepo::create("code-software-filter");
    repo.write(
        "src/Cargo.toml",
        r#"
[package]
name = "fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
"#,
    );
    repo.write("src/lib.rs", "pub fn filtered_dependency_manifest() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "filtered software manifest"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(
        &service,
        &repo,
        vec!["src".to_owned()],
        "register-software-filter",
    )
    .await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-software-filter"),
        )
        .await
        .expect("filtered scope should index");

    let projection = service
        .software_global_projection(
            SoftwareGlobalRequest::new(
                filtered_selector("fixture", "HEAD", vec!["./src/".to_owned()]),
                SoftwareGlobalKind::Dependencies,
                FreshnessPolicy::WaitUntilFresh,
                10,
            )
            .expect("software request should validate"),
            context("software-filter"),
        )
        .await
        .expect("canonical filter spelling should match indexed software scope");

    assert_eq!(projection.scope.path_filters, ["src"]);
    assert!(
        projection
            .components
            .iter()
            .any(|component| component.name == "serde")
    );
}

#[tokio::test]
async fn software_projection_allow_stale_serves_completed_scope_during_active_index() {
    let repo = FixtureRepo::create("code-software-active");
    repo.write(
        "Cargo.toml",
        r#"
[package]
name = "fixture"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1"
"#,
    );
    repo.write("src/lib.rs", "pub fn old_dependency_manifest() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "old software manifest"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, Vec::new(), "register-software-active").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-software-active-old"),
        )
        .await
        .expect("old head should index");

    repo.write(
        "Cargo.toml",
        r#"
[package]
name = "fixture"
version = "0.2.0"
edition = "2021"

[dependencies]
tokio = "1"
"#,
    );
    repo.write("src/lib.rs", "pub fn new_dependency_manifest() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "new software manifest"]);
    service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::AllowStale,
            },
            context("queue-software-active-new"),
        )
        .await
        .expect("new head should queue");

    let fresh_error = software_projection(&service, "HEAD", FreshnessPolicy::WaitUntilFresh)
        .await
        .expect_err("wait-until-fresh should reject stale code scope");
    assert!(
        fresh_error.message.contains("is stale") || fresh_error.message.contains("has no index")
    );

    let projection = software_projection(&service, "HEAD", FreshnessPolicy::AllowStale)
        .await
        .expect("allow-stale should serve completed projection");

    assert!(projection.metadata.stale);
    assert!(projection.scope.stale);
    assert!(projection.status.stale);
    assert!(
        projection
            .components
            .iter()
            .any(|component| component.name == "serde")
    );
    assert!(
        !projection
            .components
            .iter()
            .any(|component| component.name == "tokio")
    );
}

#[tokio::test]
async fn software_projection_links_document_topics_config_and_code_files() {
    let repo = FixtureRepo::create("code-software-doc-config");
    repo.write(
        "docs/runtime.md",
        "# Runtime Configuration\n\n`payments.enabled` controls checkout rollout.\n",
    );
    repo.write("config/flags.yaml", "payments:\n  enabled: true\n");
    repo.write(
        "src/lib.rs",
        "pub fn checkout_enabled(config: &Config) -> bool {\n    config.get_bool(\"payments.enabled\")\n}\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "document config software graph"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, Vec::new(), "register-software-doc-config").await;

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-software-doc-config"),
        )
        .await
        .expect("doc/config scope should index");
    let projection = software_projection(&service, "HEAD", FreshnessPolicy::WaitUntilFresh)
        .await
        .expect("software projection should load");

    assert!(
        projection
            .files
            .iter()
            .any(|file| { file.path == "docs/runtime.md" && file.file_role == "documentation" })
    );
    assert!(
        projection
            .topics
            .iter()
            .any(|topic| topic.name == "Runtime Configuration")
    );
    assert!(projection.relationships.iter().any(|relationship| {
        relationship.relationship_kind == "documents"
            && relationship.evidence_path == "docs/runtime.md"
    }));
    assert!(projection.relationships.iter().any(|relationship| {
        relationship.relationship_kind == "configures"
            && relationship.target_hint.as_deref() == Some("payments.enabled")
    }));
}

#[tokio::test]
async fn moved_branch_requires_new_scope_and_queries_rebased_head() {
    let repo = FixtureRepo::create("code-rebase-scope");
    repo.write("src/lib.rs", "pub fn old_topic_policy() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "old topic"]);
    repo.git(["branch", "topic"]);
    let old_commit = repo.git_text(["rev-parse", "topic"]);
    repo.write("src/lib.rs", "pub fn new_topic_policy() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "new topic"]);
    repo.git(["branch", "-f", "topic", "HEAD"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, vec!["src".to_owned()], "register-rebase").await;

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", &old_commit),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-old-topic"),
        )
        .await
        .expect("old topic commit should index");
    let stale_topic = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "old_topic_policy",
                selector("fixture", "topic"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-topic-before-new-index"),
        )
        .await
        .expect_err("moved branch should require indexing the new snapshot");

    assert!(stale_topic.message.contains("no index for ref"));

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "topic"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-new-topic"),
        )
        .await
        .expect("new topic should index");
    let topic = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "new_topic_policy",
                selector("fixture", "topic"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-new-topic"),
        )
        .await
        .expect("topic should read new scope");

    assert!(
        topic
            .results
            .iter()
            .any(|hit| hit.excerpt.contains("new_topic_policy"))
    );
    assert!(
        !topic
            .results
            .iter()
            .any(|hit| hit.excerpt.contains("old_topic_policy"))
    );
}

async fn software_projection(
    service: &RelayKnowledgeService,
    ref_selector: &str,
    freshness_policy: FreshnessPolicy,
) -> Result<relay_knowledge::api::SoftwareGlobalResponse, relay_knowledge::api::ApiError> {
    service
        .software_global_projection(
            SoftwareGlobalRequest::new(
                selector("fixture", ref_selector),
                SoftwareGlobalKind::All,
                freshness_policy,
                10,
            )
            .expect("software request should validate"),
            context("software"),
        )
        .await
}

async fn register_fixture_repo(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    path_filters: Vec<String>,
    name: &str,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters,
                language_filters: Vec::new(),
            },
            context(name),
        )
        .await
        .expect("repository should register");
}

fn selector(alias: &str, ref_selector: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new(alias, ref_selector, Vec::new(), Vec::new())
        .expect("selector should validate")
}

fn filtered_selector(
    alias: &str,
    ref_selector: &str,
    path_filters: Vec<String>,
) -> CodeRepositorySelector {
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
