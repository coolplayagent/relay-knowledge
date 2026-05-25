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
async fn queries_can_narrow_a_full_repository_index_by_path_or_language() {
    let repo = FixtureRepo::create("code-query-filter-narrowing");
    repo.write(
        "src/lib.rs",
        r#"
pub fn retry_policy() -> u32 {
    3
}
"#,
    );
    repo.write(
        "tests/helper.rs",
        r#"
pub fn test_retry_policy() -> u32 {
    retry_policy()
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-filter-narrowing"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-filter-narrowing"),
        )
        .await
        .expect("repository should index");

    let path_response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy",
                CodeRepositorySelector::new(
                    "fixture",
                    "HEAD",
                    vec!["src/lib.rs".to_owned()],
                    Vec::new(),
                )
                .expect("selector should validate"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-path-filter-narrowing"),
        )
        .await
        .expect("path-filtered query should use the full index");
    let rust_response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy",
                CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), vec!["rust".to_owned()])
                    .expect("selector should validate"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-language-filter-narrowing"),
        )
        .await
        .expect("language-filtered query should use the full index");
    let python_response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy",
                CodeRepositorySelector::new(
                    "fixture",
                    "HEAD",
                    Vec::new(),
                    vec!["python".to_owned()],
                )
                .expect("selector should validate"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-language-filter-empty"),
        )
        .await
        .expect("non-matching language should return an empty result set");

    assert!(
        path_response
            .results
            .iter()
            .all(|hit| hit.path == "src/lib.rs")
    );
    assert!(
        rust_response
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert!(python_response.results.is_empty());
}

#[tokio::test]
async fn language_scoped_index_includes_dependency_manifests() {
    let repo = FixtureRepo::create("code-query-language-sbom");
    repo.write("src/lib.rs", "pub fn uses_serde() {}\n");
    repo.write("Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: Vec::new(),
                language_filters: vec!["rust".to_owned()],
            },
            context("register-language-sbom"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-language-sbom"),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "serde",
                selector("fixture", "HEAD"),
                CodeQueryKind::Sbom,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-language-sbom"),
        )
        .await
        .expect("sbom query should include language-compatible manifests");

    assert!(response.results.iter().any(|hit| {
        hit.path == "Cargo.toml" && hit.edge_target_hint.as_deref() == Some("serde")
    }));
}

#[tokio::test]
async fn restricted_index_rejects_query_filters_outside_indexed_scope() {
    let repo = FixtureRepo::create("code-query-filter-restricted");
    repo.write(
        "src/lib.rs",
        r#"
pub fn retry_policy() -> u32 {
    3
}
"#,
    );
    repo.write(
        "tests/helper.rs",
        r#"
pub fn test_retry_policy() -> u32 {
    retry_policy()
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: vec!["rust".to_owned()],
            },
            context("register-restricted"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-restricted"),
        )
        .await
        .expect("repository should index");

    let narrower_response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy",
                CodeRepositorySelector::new(
                    "fixture",
                    "HEAD",
                    vec!["src/lib.rs".to_owned()],
                    Vec::new(),
                )
                .expect("selector should validate"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-restricted-narrower"),
        )
        .await
        .expect("narrower filter should use the indexed base scope");
    let path_error = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy",
                CodeRepositorySelector::new(
                    "fixture",
                    "HEAD",
                    vec!["tests".to_owned()],
                    Vec::new(),
                )
                .expect("selector should validate"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-restricted-path"),
        )
        .await
        .expect_err("path outside indexed scope should be rejected");
    let language_error = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "retry_policy",
                CodeRepositorySelector::new(
                    "fixture",
                    "HEAD",
                    Vec::new(),
                    vec!["python".to_owned()],
                )
                .expect("selector should validate"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-restricted-language"),
        )
        .await
        .expect_err("language outside indexed scope should be rejected");

    assert!(
        narrower_response
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert!(path_error.message.contains("requested filters"));
    assert!(language_error.message.contains("requested filters"));
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
