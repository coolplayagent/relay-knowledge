use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector, CodeRetrievalLayer,
        CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[tokio::test]
async fn reference_query_uses_ripgrep_text_fallback_for_comment_reference() {
    if Command::new("rg").arg("--version").output().is_err() {
        return;
    }
    let repo = FixtureRepo::create("code-ripgrep-reference");
    repo.write(
        "include/macros.h",
        "#ifndef RK_MACROS_H\n#define RK_MACROS_H\n#define RK_TRACE_VALUE(value) ((value) + 17)\n#endif\n",
    );
    repo.write(
        "src/main.c",
        "#include \"../include/macros.h\"\n// RK_TRACE_NOTE documents fallback-only macro text.\nint read_value(int input) {\n    return RK_TRACE_VALUE(input);\n}\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "macro reference"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["include".to_owned(), "src".to_owned()],
                language_filters: vec!["c".to_owned()],
            },
            context("register-ripgrep-reference"),
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
            context("index-ripgrep-reference"),
        )
        .await
        .expect("repository should index");

    let response = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "RK_TRACE_NOTE",
                CodeRepositorySelector::new(
                    "fixture",
                    "HEAD",
                    vec!["src/main.c".to_owned()],
                    vec!["c".to_owned()],
                )
                .expect("selector should validate"),
                CodeQueryKind::References,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-ripgrep-reference"),
        )
        .await
        .expect("query should succeed");

    let hit = response
        .results
        .iter()
        .find(|hit| {
            hit.excerpt
                .contains("RK_TRACE_NOTE documents fallback-only macro text")
        })
        .expect("comment reference should be recovered");
    assert!(hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical));
    assert!(
        hit.retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
    assert!(hit.edge_confidence_basis_points.is_none());
    assert!(hit.edge_confidence_tier.is_none());
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
        fs::create_dir_all(&path).expect("repo directory should be created");
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
        let output = Command::new("git")
            .current_dir(&self.path)
            .args(args)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for FixtureRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
