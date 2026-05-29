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
async fn query_language_filter_includes_dependency_manifests() {
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
                language_filters: Vec::new(),
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
async fn query_language_filters_preserve_shared_dependency_manifest_languages() {
    assert_language_scoped_sbom(LanguageScopedSbomFixture {
        repo_name: "code-query-typescript-sbom",
        alias: "fixture-ts",
        source_path: "src/app.ts",
        source_content: "export const usesReact = true;\n",
        manifest_path: "package.json",
        manifest_content: r#"{"dependencies":{"react":"^18"}}"#,
        language: "typescript",
        dependency_query: "react",
    })
    .await;
    assert_language_scoped_sbom(LanguageScopedSbomFixture {
        repo_name: "code-query-kotlin-sbom",
        alias: "fixture-kotlin",
        source_path: "src/main/kotlin/App.kt",
        source_content: "fun main() = println(\"ok\")\n",
        manifest_path: "build.gradle.kts",
        manifest_content: r#"dependencies { implementation("org.slf4j:slf4j-api:2.0.9") }"#,
        language: "kotlin",
        dependency_query: "org.slf4j:slf4j-api",
    })
    .await;
    assert_language_scoped_sbom(LanguageScopedSbomFixture {
        repo_name: "code-query-c-sbom",
        alias: "fixture-c",
        source_path: "src/app.c",
        source_content: "int main(void) { return 0; }\n",
        manifest_path: "conanfile.txt",
        manifest_content: "[requires]\nzlib/1.2.13\n",
        language: "c",
        dependency_query: "zlib",
    })
    .await;
    assert_language_scoped_sbom(LanguageScopedSbomFixture {
        repo_name: "code-query-cmake-sbom",
        alias: "fixture-cmake",
        source_path: "src/app.cpp",
        source_content: "int main() { return 0; }\n",
        manifest_path: "CMakeLists.txt",
        manifest_content: "find_package(ZLIB REQUIRED)\n",
        language: "cpp",
        dependency_query: "ZLIB",
    })
    .await;
    assert_language_scoped_sbom(LanguageScopedSbomFixture {
        repo_name: "code-query-yaml-sbom",
        alias: "fixture-yaml",
        source_path: "config/app.yaml",
        source_content: "service:\n  enabled: true\n",
        manifest_path: ".github/workflows/ci.yml",
        manifest_content: "jobs:\n  build:\n    steps:\n      - uses: actions/checkout@v4\n",
        language: "yaml",
        dependency_query: "actions/checkout",
    })
    .await;
}

#[tokio::test]
async fn query_configuration_languages_returns_nested_keys() {
    let repo = FixtureRepo::create("code-query-config-languages");
    repo.write(
        "config/app.yaml",
        "services:\n  api:\n    image: ghcr.io/org/app:1.2.3\n",
    );
    repo.write(
        "config/application.properties",
        "spring.datasource.url=jdbc:postgresql://localhost/app\n",
    );
    repo.write(
        "package.json",
        r#"{"scripts":{"build":"vite build"},"dependencies":{"react":"^18"}}"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture-config".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context("register-config-languages"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture-config", "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-config-languages"),
        )
        .await
        .expect("repository should index");

    let yaml = query_language_key(&service, "fixture-config", "services.api.image", "yaml").await;
    assert!(yaml.results.iter().any(|hit| hit.path == "config/app.yaml"));
    let properties = query_language_key(
        &service,
        "fixture-config",
        "spring.datasource.url",
        "properties",
    )
    .await;
    assert!(
        properties
            .results
            .iter()
            .any(|hit| hit.path == "config/application.properties")
    );
    let json = query_language_key(&service, "fixture-config", "scripts.build", "json").await;
    assert!(json.results.iter().any(|hit| hit.path == "package.json"));
}

async fn query_language_key(
    service: &RelayKnowledgeService,
    alias: &str,
    query: &str,
    language: &str,
) -> relay_knowledge::api::CodeRepositoryQueryResponse {
    service
        .query_code_repository(
            CodeRetrievalRequest::new(
                query,
                CodeRepositorySelector::new(alias, "HEAD", Vec::new(), vec![language.to_owned()])
                    .expect("selector should validate"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context(&format!("query-config-{language}")),
        )
        .await
        .expect("configuration key query should succeed")
}

#[tokio::test]
async fn restricted_path_index_rejects_query_paths_outside_indexed_scope() {
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
                language_filters: Vec::new(),
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
    assert!(
        narrower_response
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
    assert!(path_error.message.contains("requested filters"));
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

async fn assert_language_scoped_sbom(fixture: LanguageScopedSbomFixture<'_>) {
    let repo = FixtureRepo::create(fixture.repo_name);
    repo.write(fixture.source_path, fixture.source_content);
    repo.write(fixture.manifest_path, fixture.manifest_content);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: fixture.alias.to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context(&format!("register-{}", fixture.repo_name)),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector(fixture.alias, "HEAD"),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context(&format!("index-{}", fixture.repo_name)),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                fixture.dependency_query,
                CodeRepositorySelector::new(
                    fixture.alias,
                    "HEAD",
                    Vec::new(),
                    vec![fixture.language.to_owned()],
                )
                .expect("selector should validate"),
                CodeQueryKind::Sbom,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context(&format!("query-{}", fixture.repo_name)),
        )
        .await
        .expect("sbom query should include language-compatible manifests");

    assert!(response.results.iter().any(|hit| {
        hit.path == fixture.manifest_path
            && hit.language_id == fixture.language
            && hit.edge_target_hint.as_deref() == Some(fixture.dependency_query)
    }));
}

struct LanguageScopedSbomFixture<'a> {
    repo_name: &'a str,
    alias: &'a str,
    source_path: &'a str,
    source_content: &'a str,
    manifest_path: &'a str,
    manifest_content: &'a str,
    language: &'a str,
    dependency_query: &'a str,
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
