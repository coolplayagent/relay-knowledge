use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;
use crate::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind},
    application::RuntimeConfiguration,
    domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector},
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[test]
fn parses_repo_set_commands_and_validation_errors() {
    assert_eq!(
        parse_repo_set(&[
            "create".to_owned(),
            "workspace".to_owned(),
            "--description".to_owned(),
            "all repos".to_owned(),
        ])
        .expect("create should parse"),
        RepoSetCommand::Create {
            alias: "workspace".to_owned(),
            description: Some("all repos".to_owned()),
        }
    );
    assert_eq!(
        parse_repo_set(&[
            "add".to_owned(),
            "workspace".to_owned(),
            "core".to_owned(),
            "--ref".to_owned(),
            "main".to_owned(),
            "--path".to_owned(),
            "src".to_owned(),
            "--language".to_owned(),
            "rust".to_owned(),
            "--priority".to_owned(),
            "4".to_owned(),
        ])
        .expect("add should parse"),
        RepoSetCommand::Add {
            set_alias: "workspace".to_owned(),
            repository_alias: "core".to_owned(),
            ref_selector: "main".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            priority: 4,
        }
    );
    assert_eq!(
        parse_query_kind("sbom").expect("sbom query kind should parse"),
        CodeQueryKind::Sbom
    );
    assert_eq!(
        parse_repo_set(&[
            "remove".to_owned(),
            "workspace".to_owned(),
            "core".to_owned(),
        ])
        .expect("remove should parse"),
        RepoSetCommand::Remove {
            set_alias: "workspace".to_owned(),
            repository_alias: "core".to_owned(),
        }
    );
    assert_eq!(
        parse_repo_set(&[
            "query".to_owned(),
            "workspace".to_owned(),
            "--query".to_owned(),
            "RetryPolicy".to_owned(),
            "builder".to_owned(),
            "--kind".to_owned(),
            "references".to_owned(),
            "--limit".to_owned(),
            "7".to_owned(),
            "--path".to_owned(),
            "src".to_owned(),
            "--language".to_owned(),
            "rust".to_owned(),
            "--freshness".to_owned(),
            "wait-until-fresh".to_owned(),
        ])
        .expect("query should parse"),
        RepoSetCommand::Query {
            set_alias: "workspace".to_owned(),
            query: "RetryPolicy builder".to_owned(),
            kind: CodeQueryKind::References,
            limit: 7,
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            freshness: FreshnessPolicy::WaitUntilFresh,
            exclude_generated: false,
        }
    );
    assert!(matches!(
        parse_repo_set(&[
            "query".to_owned(),
            "workspace".to_owned(),
            "--query".to_owned(),
            "RetryPolicy".to_owned(),
            "--exclude-generated".to_owned(),
        ])
        .expect("query should parse generated exclusion"),
        RepoSetCommand::Query {
            exclude_generated: true,
            ..
        }
    ));
    assert_eq!(
        parse_repo_set(&[
            "query".to_owned(),
            "workspace".to_owned(),
            "serve".to_owned()
        ])
        .expect("positional query should parse"),
        RepoSetCommand::Query {
            set_alias: "workspace".to_owned(),
            query: "serve".to_owned(),
            kind: CodeQueryKind::Hybrid,
            limit: 10,
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
            exclude_generated: false,
        }
    );
    assert_eq!(
        parse_repo_set(&["status".to_owned(), "workspace".to_owned()])
            .expect("status should parse"),
        RepoSetCommand::Status {
            set_alias: "workspace".to_owned(),
        }
    );
    assert_eq!(
        parse_repo_set(&[
            "refresh".to_owned(),
            "workspace".to_owned(),
            "--async".to_owned(),
        ])
        .expect("refresh should parse"),
        RepoSetCommand::Refresh {
            set_alias: "workspace".to_owned(),
            async_task: true,
        }
    );
    assert_eq!(
        parse_repo_set(&[
            "refresh-worker".to_owned(),
            "--task-id".to_owned(),
            "task-1".to_owned(),
        ])
        .expect("worker should parse"),
        RepoSetCommand::RefreshWorker {
            task_id: Some("task-1".to_owned()),
        }
    );

    assert_eq!(
        parse_repo_set(&[]).expect_err("empty command should fail"),
        CliError::UnexpectedArgument("repo-set".to_owned())
    );
    assert_eq!(
        parse_repo_set(&["unknown".to_owned()]).expect_err("unknown command should fail"),
        CliError::UnexpectedArgument("unknown".to_owned())
    );
    assert_eq!(
        parse_repo_set(&["add".to_owned(), "workspace".to_owned(), "core".to_owned()])
            .expect_err("missing ref should fail"),
        CliError::MissingValue("--ref")
    );
    assert_eq!(
        parse_repo_set(&["remove".to_owned(), "workspace".to_owned()])
            .expect_err("missing remove alias should fail"),
        CliError::MissingValue("<repo-alias>")
    );
    assert_eq!(
        parse_repo_set(&[
            "query".to_owned(),
            "workspace".to_owned(),
            "--kind".to_owned(),
            "unknown".to_owned(),
            "--query".to_owned(),
            "x".to_owned(),
        ])
        .expect_err("unknown query kind should fail"),
        CliError::InvalidCodeQueryKind("unknown".to_owned())
    );
    assert_eq!(
        parse_repo_set(&[
            "query".to_owned(),
            "workspace".to_owned(),
            "--query".to_owned(),
            "x".to_owned(),
            "--limit".to_owned(),
            "many".to_owned(),
        ])
        .expect_err("invalid limit should fail"),
        CliError::InvalidLimit("many".to_owned())
    );
}

#[tokio::test]
async fn runs_repo_set_commands_against_shared_service() {
    let app_repo = FixtureRepo::create("repo-set-cli-app");
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
    let service_repo = FixtureRepo::create("repo-set-cli-service");
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

    let created = run_repo_set(
        &service,
        RepoSetCommand::Create {
            alias: "workspace".to_owned(),
            description: Some("cli set".to_owned()),
        },
        context("create"),
        OutputFormat::Json,
    )
    .await
    .expect("set create should run");
    assert_eq!(json_value(&created)["repository_set"]["alias"], "workspace");

    for alias in ["app", "svc"] {
        let added = run_repo_set(
            &service,
            RepoSetCommand::Add {
                set_alias: "workspace".to_owned(),
                repository_alias: alias.to_owned(),
                ref_selector: "HEAD".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
                priority: if alias == "app" { 10 } else { 0 },
            },
            context(&format!("add-{alias}")),
            OutputFormat::Json,
        )
        .await
        .expect("set add should run");
        assert_eq!(json_value(&added)["member"]["repository_alias"], alias);
    }

    let status = run_repo_set(
        &service,
        RepoSetCommand::Status {
            set_alias: "workspace".to_owned(),
        },
        context("status"),
        OutputFormat::Json,
    )
    .await
    .expect("status should run");
    assert_eq!(
        json_value(&status)["status"]["members"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let graph_only = run_repo_set(
        &service,
        RepoSetCommand::Query {
            set_alias: "workspace".to_owned(),
            query: "serve".to_owned(),
            kind: CodeQueryKind::Definition,
            limit: 5,
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::GraphOnly,
            exclude_generated: false,
        },
        context("query-graph-only"),
        OutputFormat::Json,
    )
    .await
    .expect("graph-only query should return diagnostics");
    let graph_only = json_value(&graph_only);
    assert_eq!(graph_only["results"].as_array().unwrap().len(), 0);
    assert_eq!(
        graph_only["degraded_reason"],
        "graph_only freshness policy selected"
    );

    let stale_overlay = run_repo_set(
        &service,
        RepoSetCommand::Query {
            set_alias: "workspace".to_owned(),
            query: "serve".to_owned(),
            kind: CodeQueryKind::Definition,
            limit: 5,
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::WaitUntilFresh,
            exclude_generated: false,
        },
        context("query-wait-before-refresh"),
        OutputFormat::Json,
    )
    .await
    .expect_err("wait-until-fresh should reject stale overlay");
    assert!(
        stale_overlay
            .to_string()
            .contains("overlay is stale; run repo-set refresh")
    );

    let queried = run_repo_set(
        &service,
        RepoSetCommand::Query {
            set_alias: "workspace".to_owned(),
            query: "serve".to_owned(),
            kind: CodeQueryKind::Definition,
            limit: 5,
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
            exclude_generated: false,
        },
        context("query"),
        OutputFormat::Json,
    )
    .await
    .expect("query should run");
    assert!(
        json_value(&queried)["results"]
            .as_array()
            .expect("results should be an array")
            .iter()
            .any(|hit| hit["member"]["repository_alias"] == "svc")
    );

    let refreshed = run_repo_set(
        &service,
        RepoSetCommand::Refresh {
            set_alias: "workspace".to_owned(),
            async_task: false,
        },
        context("refresh"),
        OutputFormat::Json,
    )
    .await
    .expect("refresh should run");
    assert_eq!(json_value(&refreshed)["summary"]["resolved_edge_count"], 1);

    let queued = run_repo_set(
        &service,
        RepoSetCommand::Refresh {
            set_alias: "workspace".to_owned(),
            async_task: true,
        },
        context("refresh-async"),
        OutputFormat::Json,
    )
    .await
    .expect("async refresh should queue");
    assert_eq!(json_value(&queued)["task"]["state"], "queued");

    let completed = run_repo_set(
        &service,
        RepoSetCommand::RefreshWorker { task_id: None },
        context("refresh-worker"),
        OutputFormat::Json,
    )
    .await
    .expect("refresh worker should run");
    assert_eq!(json_value(&completed)["state"], "succeeded");

    let removed = run_repo_set(
        &service,
        RepoSetCommand::Remove {
            set_alias: "workspace".to_owned(),
            repository_alias: "app".to_owned(),
        },
        context("remove-app"),
        OutputFormat::Json,
    )
    .await
    .expect("set remove should run");
    let removed = json_value(&removed);
    assert_eq!(removed["member"]["repository_alias"], "app");
    assert_eq!(
        removed["status"]["members"]
            .as_array()
            .expect("members should be an array")
            .len(),
        1
    );
    assert_eq!(removed["status"]["overlay"]["state"], "missing");

    run_repo_set(
        &service,
        RepoSetCommand::Create {
            alias: "empty-workspace".to_owned(),
            description: None,
        },
        context("create-empty"),
        OutputFormat::Json,
    )
    .await
    .expect("empty set should create");
    let queued_empty = run_repo_set(
        &service,
        RepoSetCommand::Refresh {
            set_alias: "empty-workspace".to_owned(),
            async_task: true,
        },
        context("refresh-empty-async"),
        OutputFormat::Json,
    )
    .await
    .expect("empty async refresh should queue");
    let task_id = json_value(&queued_empty)["task"]["task_id"]
        .as_str()
        .expect("queued task should expose id")
        .to_owned();
    let failed_empty = run_repo_set(
        &service,
        RepoSetCommand::RefreshWorker {
            task_id: Some(task_id),
        },
        context("refresh-empty-worker"),
        OutputFormat::Json,
    )
    .await
    .expect_err("empty set worker should fail and persist retry");
    assert!(failed_empty.to_string().contains("has no members"));

    let idle = run_repo_set(
        &service,
        RepoSetCommand::RefreshWorker { task_id: None },
        context("refresh-worker-idle"),
        OutputFormat::Json,
    )
    .await
    .expect("idle refresh worker should run");
    assert!(idle.is_empty());
}

fn json_value(output: &str) -> serde_json::Value {
    serde_json::from_str(output.trim()).expect("json output should parse")
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

async fn register_and_index(service: &RelayKnowledgeService, repo: &FixtureRepo, alias: &str) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: alias.to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
            },
            context(&format!("register-{alias}")),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new(alias, "HEAD", Vec::new(), Vec::new())
                    .expect("selector should validate"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context(&format!("index-{alias}")),
        )
        .await
        .expect("repository should index");
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
