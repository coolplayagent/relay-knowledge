use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;
use crate::{
    api::InterfaceKind,
    application::RuntimeConfiguration,
    env::{EnvironmentConfig, PlatformKind},
    storage::SqliteGraphStore,
};

#[test]
fn parses_repo_query_with_kind_filters_and_freshness() {
    let command = parse_repo(&[
        "query".to_owned(),
        "core".to_owned(),
        "--query".to_owned(),
        "RetryPolicy".to_owned(),
        "--kind".to_owned(),
        "references".to_owned(),
        "--path".to_owned(),
        "src".to_owned(),
        "--language".to_owned(),
        "rust".to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
    ])
    .expect("repo query should parse");

    assert_eq!(
        command,
        RepoCommand::Query {
            alias: "core".to_owned(),
            query: "RetryPolicy".to_owned(),
            kind: CodeQueryKind::References,
            limit: 10,
            ref_selector: "HEAD".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            freshness: FreshnessPolicy::WaitUntilFresh,
        }
    );
}

#[test]
fn parses_repo_feature_flags_with_optional_filter_and_scope() {
    let command = parse_repo(&[
        "feature-flags".to_owned(),
        "core".to_owned(),
        "--query".to_owned(),
        "checkout".to_owned(),
        "--ref".to_owned(),
        "HEAD".to_owned(),
        "--path".to_owned(),
        "src".to_owned(),
        "--language".to_owned(),
        "rust".to_owned(),
        "--freshness".to_owned(),
        "wait-until-fresh".to_owned(),
        "--limit".to_owned(),
        "20".to_owned(),
    ])
    .expect("feature flags command should parse");

    assert_eq!(
        command,
        RepoCommand::FeatureFlags {
            alias: "core".to_owned(),
            query: Some("checkout".to_owned()),
            limit: 20,
            ref_selector: "HEAD".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            freshness: FreshnessPolicy::WaitUntilFresh,
        }
    );
}

#[test]
fn parses_repo_command_forms_and_validation_errors() {
    let register = parse_repo(&[
        "register".to_owned(),
        "/work/repo".to_owned(),
        "--alias".to_owned(),
        "core".to_owned(),
        "--path".to_owned(),
        "src".to_owned(),
    ])
    .expect("register command should parse");
    assert_eq!(
        register,
        RepoCommand::Register {
            root_path: "/work/repo".to_owned(),
            alias: "core".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: Vec::new(),
        }
    );
    assert_eq!(
        parse_repo(&["register".to_owned(), "/work/repo".to_owned()])
            .expect("register without alias should parse"),
        RepoCommand::Register {
            root_path: "/work/repo".to_owned(),
            alias: String::new(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        }
    );

    assert_eq!(
        parse_repo(&["index".to_owned(), "core".to_owned()]).expect("index command should parse"),
        RepoCommand::Index {
            alias: "core".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: false,
        }
    );
    assert_eq!(
        parse_repo(&[
            "index".to_owned(),
            "core".to_owned(),
            "--dry-run".to_owned(),
            "--ref".to_owned(),
            "main".to_owned(),
        ])
        .expect("dry-run index command should parse"),
        RepoCommand::Index {
            alias: "core".to_owned(),
            ref_selector: "main".to_owned(),
            dry_run: true,
        }
    );
    assert_eq!(
        parse_repo(&[
            "scope".to_owned(),
            "preview".to_owned(),
            "core".to_owned(),
            "--ref".to_owned(),
            "main".to_owned(),
        ])
        .expect("scope preview should parse"),
        RepoCommand::ScopePreview {
            alias: "core".to_owned(),
            ref_selector: "main".to_owned(),
        }
    );
    assert_eq!(
        parse_repo(&["report".to_owned(), "core".to_owned()]).expect("report command should parse"),
        RepoCommand::Report {
            alias: "core".to_owned()
        }
    );
    assert_eq!(
        parse_repo(&[
            "update".to_owned(),
            "core".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
            "--head".to_owned(),
            "feature".to_owned(),
        ])
        .expect("update command should parse"),
        RepoCommand::Update {
            alias: "core".to_owned(),
            base_ref: "main".to_owned(),
            head_ref: "feature".to_owned(),
        }
    );
    assert_eq!(
        parse_repo(&[
            "impact".to_owned(),
            "core".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
            "--head".to_owned(),
            "feature".to_owned(),
            "--limit".to_owned(),
            "7".to_owned(),
        ])
        .expect("impact command should parse"),
        RepoCommand::Impact {
            alias: "core".to_owned(),
            base_ref: "main".to_owned(),
            head_ref: "feature".to_owned(),
            limit: 7,
        }
    );
    assert_eq!(
        parse_repo(&["status".to_owned(), "core".to_owned()]).expect("status command should parse"),
        RepoCommand::Status {
            alias: "core".to_owned()
        }
    );

    assert_eq!(parse_query_kind("hybrid").unwrap(), CodeQueryKind::Hybrid);
    assert_eq!(parse_query_kind("symbol").unwrap(), CodeQueryKind::Symbol);
    assert_eq!(
        parse_query_kind("definition").unwrap(),
        CodeQueryKind::Definition
    );
    assert_eq!(parse_query_kind("callers").unwrap(), CodeQueryKind::Callers);
    assert_eq!(parse_query_kind("callees").unwrap(), CodeQueryKind::Callees);
    assert_eq!(parse_query_kind("imports").unwrap(), CodeQueryKind::Imports);
    assert_eq!(parse_query_kind("sbom").unwrap(), CodeQueryKind::Sbom);
    assert_eq!(
        parse_query_kind("impact").unwrap_err(),
        CliError::InvalidCodeQueryKind("impact".to_owned())
    );

    let positional_query = parse_repo(&[
        "query".to_owned(),
        "core".to_owned(),
        "RetryPolicy".to_owned(),
        "budget".to_owned(),
        "--kind".to_owned(),
        "symbol".to_owned(),
    ])
    .expect("positional query should parse");
    assert!(matches!(
        &positional_query,
        RepoCommand::Query {
            kind: CodeQueryKind::Symbol,
            ..
        }
    ));
    assert!(matches!(
        positional_query,
        RepoCommand::Query { query, .. } if query == "RetryPolicy budget"
    ));

    assert_eq!(
        parse_repo(&[]).expect_err("empty repo command should fail"),
        CliError::UnexpectedArgument("repo".to_owned())
    );
    assert_eq!(
        parse_repo(&["query".to_owned(), "core".to_owned()])
            .expect_err("missing query should fail"),
        CliError::MissingValue("--query")
    );
    assert_eq!(
        parse_repo(&[
            "impact".to_owned(),
            "core".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
        ])
        .expect_err("missing head should fail"),
        CliError::MissingValue("--head")
    );
    assert_eq!(
        parse_repo(&[
            "query".to_owned(),
            "core".to_owned(),
            "--query".to_owned(),
            "RetryPolicy".to_owned(),
            "--kind".to_owned(),
            "unknown".to_owned(),
        ])
        .expect_err("unknown query kind should fail"),
        CliError::InvalidCodeQueryKind("unknown".to_owned())
    );
    assert_eq!(
        parse_repo(&[
            "impact".to_owned(),
            "core".to_owned(),
            "--base".to_owned(),
            "main".to_owned(),
            "--head".to_owned(),
            "feature".to_owned(),
            "--limit".to_owned(),
            "many".to_owned(),
        ])
        .expect_err("bad limit should fail"),
        CliError::InvalidLimit("many".to_owned())
    );
    assert_eq!(
        parse_repo(&["unknown".to_owned()]).expect_err("unknown subcommand should fail"),
        CliError::UnexpectedArgument("unknown".to_owned())
    );
}

#[tokio::test]
async fn runs_repo_commands_against_shared_service() {
    let repo = FixtureRepo::create("repo-cli");
    repo.write(
        "src/lib.rs",
        r#"
/// Selects the retry budget.
pub fn retry_policy() -> u32 {
    3
}
"#,
    );
    repo.write(
        "src/main.rs",
        r#"
use crate::retry_policy;

fn run_worker() {
    retry_policy();
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let base_ref = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;

    let registered = run_repo(
        &service,
        RepoCommand::Register {
            root_path: repo.path.display().to_string(),
            alias: "fixture".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: Vec::new(),
        },
        context("register"),
        OutputFormat::Json,
    )
    .await
    .expect("register should run");
    let value = json_value(&registered);
    assert_eq!(value["registration"]["alias"], "fixture");

    let preview = run_repo(
        &service,
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: true,
        },
        context("preview"),
        OutputFormat::Json,
    )
    .await
    .expect("dry-run preview should run");
    assert_eq!(json_value(&preview)["preview"]["selected_file_count"], 2);

    let indexed = run_repo(
        &service,
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: false,
        },
        context("index"),
        OutputFormat::StreamingJson,
    )
    .await
    .expect("index should run");
    assert!(indexed.contains("code.repo.index"));

    let fresh_definitions = run_repo(
        &service,
        RepoCommand::Query {
            alias: "fixture".to_owned(),
            query: "retry_policy".to_owned(),
            kind: CodeQueryKind::Definition,
            limit: 5,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::WaitUntilFresh,
        },
        context("query-after-index"),
        OutputFormat::Json,
    )
    .await
    .expect("query should run immediately after repo index");
    assert_eq!(
        json_value(&fresh_definitions)["results"][0]["path"],
        "src/lib.rs"
    );

    run_repo(
        &service,
        RepoCommand::IndexWorker { task_id: None },
        context("index-worker"),
        OutputFormat::Json,
    )
    .await
    .expect("index worker should complete queued index");

    let definitions = run_repo(
        &service,
        RepoCommand::Query {
            alias: "fixture".to_owned(),
            query: "retry_policy".to_owned(),
            kind: CodeQueryKind::Definition,
            limit: 5,
            ref_selector: "HEAD".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
        },
        context("query"),
        OutputFormat::Json,
    )
    .await
    .expect("query should run");
    assert_eq!(json_value(&definitions)["results"][0]["path"], "src/lib.rs");

    let report = run_repo(
        &service,
        RepoCommand::Report {
            alias: "fixture".to_owned(),
        },
        context("report"),
        OutputFormat::Markdown,
    )
    .await
    .expect("report should run");
    assert!(report.contains("# Code Repository Report: fixture"));

    repo.write(
        "src/lib.rs",
        r#"
/// Selects the retry budget.
pub fn retry_policy() -> u32 {
    5
}

pub fn retry_policy_v2() -> u32 {
    retry_policy()
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update policy"]);
    let head_ref = repo.git_text(["rev-parse", "HEAD"]);

    let preview_after_change = run_repo(
        &service,
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: true,
        },
        context("preview-new-head"),
        OutputFormat::Json,
    )
    .await
    .expect("dry-run preview should run after head changes");
    let preview_value = json_value(&preview_after_change);
    assert_eq!(preview_value["scope"]["resolved_commit_sha"], base_ref);
    assert_eq!(preview_value["preview"]["resolved_commit_sha"], head_ref);

    let updated = run_repo(
        &service,
        RepoCommand::Update {
            alias: "fixture".to_owned(),
            base_ref: base_ref.clone(),
            head_ref: head_ref.clone(),
        },
        context("update"),
        OutputFormat::Text,
    )
    .await
    .expect("update should run");
    assert_eq!(updated, "code.repo.update\n");

    let impact = run_repo(
        &service,
        RepoCommand::Impact {
            alias: "fixture".to_owned(),
            base_ref,
            head_ref,
            limit: 10,
        },
        context("impact"),
        OutputFormat::Json,
    )
    .await
    .expect("impact should run");
    assert_eq!(
        json_value(&impact)["path_groups"]["in_scope_changed_paths"][0],
        "src/lib.rs"
    );

    let status = run_repo(
        &service,
        RepoCommand::Status {
            alias: "fixture".to_owned(),
        },
        context("status"),
        OutputFormat::StreamingJson,
    )
    .await
    .expect("status should run");
    assert!(status.contains("code.repo.status"));
}

#[tokio::test]
async fn default_register_alias_uses_project_name_and_survives_session_aliases() {
    let repo = FixtureRepo::create("repo-cli-default-alias");
    repo.write(
        "src/lib.rs",
        r#"
pub fn stable_project_entry() -> &'static str {
    "ready"
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let default_alias = repo
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("fixture root should have a directory name")
        .to_owned();
    let service = service_with_memory_store().await;

    let registered = run_repo(
        &service,
        RepoCommand::Register {
            root_path: repo.path.display().to_string(),
            alias: String::new(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        },
        context("register-default-alias"),
        OutputFormat::Json,
    )
    .await
    .expect("default alias registration should run");
    assert_eq!(
        json_value(&registered)["registration"]["alias"],
        default_alias
    );

    run_repo(
        &service,
        RepoCommand::Register {
            root_path: repo.path.display().to_string(),
            alias: "session-generated-alias".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        },
        context("register-session-alias"),
        OutputFormat::Json,
    )
    .await
    .expect("secondary alias registration should run");

    run_repo(
        &service,
        RepoCommand::Index {
            alias: default_alias.clone(),
            ref_selector: "HEAD".to_owned(),
            dry_run: false,
        },
        context("index-default-alias"),
        OutputFormat::Json,
    )
    .await
    .expect("index should run through default alias");

    for alias in [default_alias, "session-generated-alias".to_owned()] {
        let output = run_repo(
            &service,
            RepoCommand::Query {
                alias,
                query: "stable_project_entry".to_owned(),
                kind: CodeQueryKind::Definition,
                limit: 5,
                ref_selector: "HEAD".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
                freshness: FreshnessPolicy::AllowStale,
            },
            context("query-alias"),
            OutputFormat::Json,
        )
        .await
        .expect("query should run through each alias");
        assert_eq!(json_value(&output)["results"][0]["path"], "src/lib.rs");
    }
}

#[tokio::test]
async fn repo_register_rejects_language_filters() {
    let repo = FixtureRepo::create("repo-register-language-rejected");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    let error = run_repo(
        &service,
        RepoCommand::Register {
            root_path: repo.path.display().to_string(),
            alias: "fixture".to_owned(),
            path_filters: Vec::new(),
            language_filters: vec!["rust".to_owned()],
        },
        context("register-language-rejected"),
        OutputFormat::Json,
    )
    .await
    .expect_err("register --language should be rejected");

    assert!(
        error
            .to_string()
            .contains("registration language filters are not supported")
    );
}

#[tokio::test]
async fn repo_api_errors_render_json_stderr_when_json_format_is_requested() {
    let service = service_with_memory_store().await;
    let error = run_repo(
        &service,
        RepoCommand::Status {
            alias: "missing".to_owned(),
        },
        context("missing-repo-json"),
        OutputFormat::Json,
    )
    .await
    .expect_err("missing repository should fail");
    let value: serde_json::Value =
        serde_json::from_str(&error.render_stderr()).expect("stderr should be JSON");

    assert_eq!(value["error_kind"], "invalid_argument");
    assert_eq!(
        value["message"],
        "code repository 'missing' is not registered"
    );
    assert_eq!(error.exit_code(), 1);
}

#[tokio::test]
async fn repo_api_errors_keep_text_stderr_for_text_format() {
    let service = service_with_memory_store().await;
    let error = run_repo(
        &service,
        RepoCommand::Status {
            alias: "missing".to_owned(),
        },
        context("missing-repo-text"),
        OutputFormat::Text,
    )
    .await
    .expect_err("missing repository should fail");

    assert_eq!(
        error.render_stderr(),
        "code repository 'missing' is not registered"
    );
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
