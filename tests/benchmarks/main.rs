use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeImpactRequest, CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector,
        CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn benchmark_code_repository_fast_paths() {
    let repo = FixtureRepo::create("benchmark-code-fast-paths");
    for index in 0..24 {
        repo.write(
            &format!("src/module_{index}.rs"),
            &format!(
                "pub fn benchmark_symbol_{index}() -> u32 {{ {index} }}\n\
                 pub fn benchmark_caller_{index}() -> u32 {{ benchmark_symbol_{index}() }}\n"
            ),
        );
    }
    repo.write(
        "src/lib.rs",
        "pub mod module_0;\npub fn benchmark_entry() -> u32 { module_0::benchmark_symbol_0() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    register_repo(&service, &repo).await;

    let full_index = timed("full index", Duration::from_secs(10), || async {
        service
            .index_code_repository(
                CodeIndexRequest {
                    repository: selector("bench", &base),
                    mode: CodeIndexMode::Full,
                    freshness_policy: FreshnessPolicy::WaitUntilFresh,
                },
                context("benchmark-full-index"),
            )
            .await
            .expect("full index should complete")
    })
    .await;
    assert_eq!(full_index.summary.indexed_file_count, 25);

    repo.write(
        "src/module_0.rs",
        "pub fn benchmark_symbol_0() -> u32 { 100 }\n\
         pub fn benchmark_caller_0() -> u32 { benchmark_symbol_0() }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "head"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);

    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("bench", &head),
                mode: CodeIndexMode::Full,
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("benchmark-active-head"),
        )
        .await
        .expect("head full index should seed active head");

    let incremental = timed(
        "persisted-base incremental",
        Duration::from_secs(5),
        || async {
            service
                .index_code_repository(
                    CodeIndexRequest {
                        repository: selector("bench", &head),
                        mode: CodeIndexMode::incremental(base.clone(), head.clone())
                            .expect("incremental mode should validate"),
                        freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    },
                    context("benchmark-incremental"),
                )
                .await
                .expect("incremental update should use persisted base scope")
        },
    )
    .await;
    assert_eq!(incremental.summary.changed_path_count, 1);
    assert_eq!(incremental.summary.progress.blob_read_count, 1);
    assert_eq!(incremental.summary.progress.parsed_file_count, 1);

    let noop = timed("no-op full index", Duration::from_secs(2), || async {
        service
            .index_code_repository(
                CodeIndexRequest {
                    repository: selector("bench", &head),
                    mode: CodeIndexMode::Full,
                    freshness_policy: FreshnessPolicy::WaitUntilFresh,
                },
                context("benchmark-noop"),
            )
            .await
            .expect("no-op full index should reuse fresh scope")
    })
    .await;
    assert_eq!(noop.summary.changed_path_count, 0);
    assert_eq!(noop.summary.progress.blob_read_count, 0);
    assert_eq!(noop.summary.progress.sqlite_write_count, 0);

    let query = timed("hybrid query", Duration::from_secs(2), || async {
        service
            .query_code_repository(
                CodeRetrievalRequest::new(
                    "benchmark_symbol_0",
                    selector("bench", &head),
                    CodeQueryKind::Hybrid,
                    10,
                    FreshnessPolicy::WaitUntilFresh,
                )
                .expect("query should validate"),
                context("benchmark-query"),
            )
            .await
            .expect("query should complete")
    })
    .await;
    assert!(
        query
            .results
            .iter()
            .any(|hit| hit.path == "src/module_0.rs")
    );

    let impact = timed("impact analysis", Duration::from_secs(2), || async {
        service
            .impact_code_repository(
                CodeImpactRequest::new(selector("bench", &head), base, head, 20)
                    .expect("impact should validate"),
                context("benchmark-impact"),
            )
            .await
            .expect("impact should complete")
    })
    .await;
    assert!(
        impact
            .path_groups
            .in_scope_changed_paths
            .iter()
            .any(|path| path == "src/module_0.rs")
    );
    assert!(
        impact
            .results
            .iter()
            .any(|hit| hit.path == "src/module_0.rs")
    );
}

async fn timed<T, F, Fut>(name: &str, budget: Duration, operation: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let started = Instant::now();
    let output = operation().await;
    let elapsed = started.elapsed();
    assert!(
        elapsed <= budget,
        "{name} exceeded benchmark budget: elapsed={elapsed:?}, budget={budget:?}"
    );
    output
}

async fn register_repo(service: &RelayKnowledgeService, repo: &FixtureRepo) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "bench".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("benchmark-register"),
        )
        .await
        .expect("repository should register");
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

    fn write(&self, path: &str, content: &str) {
        let full_path = self.path.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("parent directories should exist");
        }
        fs::write(full_path, content).expect("file should be written");
    }

    fn git<const N: usize>(&self, args: [&str; N]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git command failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git output should be utf8")
            .trim()
            .to_owned()
    }
}

impl Drop for FixtureRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
