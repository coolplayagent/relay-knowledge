use super::super::test_support::{FixtureRepo, context, json_value, service_with_memory_store};
use super::*;
use crate::domain::CodeQueryKind;

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

    let registered_only = run_repo(
        &service,
        RepoCommand::List,
        context("list-before-index"),
        OutputFormat::Json,
    )
    .await
    .expect("repository list should run before indexing");
    assert!(
        json_value(&registered_only)["repositories"]
            .as_array()
            .expect("repositories should be an array")
            .is_empty()
    );

    let preview = run_repo(
        &service,
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: true,
            reuse_historical: false,
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
            reuse_historical: false,
        },
        context("index"),
        OutputFormat::StreamingJson,
    )
    .await
    .expect("index should run");
    assert!(indexed.contains("code.repo.index"));

    let listed = run_repo(
        &service,
        RepoCommand::List,
        context("list-after-index"),
        OutputFormat::Text,
    )
    .await
    .expect("repository list should include completed indexes");
    assert!(listed.contains("repositories=1"));
    assert!(listed.contains("repo=fixture state=fresh"));

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
            exclude_generated: false,
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

    let idle_worker = run_repo(
        &service,
        RepoCommand::IndexWorker { task_id: None },
        context("index-worker"),
        OutputFormat::Json,
    )
    .await
    .expect("index worker should complete queued index");
    let idle_worker_value = json_value(&idle_worker);
    assert_eq!(idle_worker_value["claimed"], false);
    assert!(idle_worker_value["task"].is_null());
    assert_eq!(idle_worker_value["maintenance_active"], false);
    assert!(idle_worker_value.get("maintenance_error").is_none());
    let idle_worker_stream = run_repo(
        &service,
        RepoCommand::IndexWorker { task_id: None },
        context("index-worker-stream"),
        OutputFormat::StreamingJson,
    )
    .await
    .expect("streaming index worker should return events");
    let stream_events = idle_worker_stream
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("event should be JSON"))
        .collect::<Vec<_>>();
    assert_eq!(stream_events.len(), 3);
    assert_eq!(stream_events[0]["event"], "started");
    assert_eq!(stream_events[1]["event"], "item");
    assert_eq!(stream_events[1]["payload"]["claimed"], false);
    assert!(stream_events[1]["payload"]["task"].is_null());
    assert_eq!(stream_events[1]["payload"]["maintenance_active"], false);
    assert!(
        stream_events[1]["payload"]
            .get("maintenance_error")
            .is_none()
    );
    assert_eq!(stream_events[2]["event"], "completed");

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
            exclude_generated: false,
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
            reuse_historical: false,
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
            base_ref: Some(base_ref.clone()),
            head_ref: Some(head_ref.clone()),
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
            base_ref: base_ref.clone(),
            head_ref: head_ref.clone(),
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

    repo.write("src/lib.rs", "pub fn retry_policy_v3() -> u32 { 7 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "third policy"]);
    let third_head = repo.git_text(["rev-parse", "HEAD"]);
    let automatic_update = run_repo(
        &service,
        RepoCommand::Update {
            alias: "fixture".to_owned(),
            base_ref: None,
            head_ref: None,
        },
        context("automatic-update"),
        OutputFormat::Json,
    )
    .await
    .expect("automatic refs should update");
    let automatic_update = json_value(&automatic_update);
    assert_eq!(
        automatic_update["task"]["mode"]["incremental"]["base_ref"],
        head_ref
    );
    assert_eq!(automatic_update["task"]["resolved_commit_sha"], third_head);
    assert_eq!(
        automatic_update["summary"]["base_resolved_commit_sha"],
        head_ref
    );
    assert_eq!(
        automatic_update["summary"]["resolved_commit_sha"],
        third_head
    );
    assert_eq!(
        automatic_update["status"]["last_indexed_commit"],
        third_head
    );

    let maintenance_worker = run_repo(
        &service,
        RepoCommand::IndexWorker { task_id: None },
        context("index-worker-retention"),
        OutputFormat::Json,
    )
    .await
    .expect("idle index worker should advance bounded scope maintenance");
    let maintenance_worker = json_value(&maintenance_worker);
    assert_eq!(maintenance_worker["claimed"], false);
    assert_eq!(maintenance_worker["maintenance_active"], true);
    assert!(maintenance_worker.get("maintenance_error").is_none());

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

    let removed = run_repo(
        &service,
        RepoCommand::Remove {
            alias: "fixture".to_owned(),
        },
        context("remove"),
        OutputFormat::Json,
    )
    .await
    .expect("remove should run");
    let removed_value = json_value(&removed);
    assert_eq!(
        removed_value["summary"]["repository_id"],
        removed_value["removed_status"]["repository_id"]
    );
    assert_eq!(removed_value["removed_status"]["alias"], "fixture");
    assert!(
        removed_value["summary"]["removed_scope_count"]
            .as_u64()
            .is_some_and(|count| count >= 2)
    );
    assert!(
        removed_value["summary"]["removed_index_task_count"]
            .as_u64()
            .is_some_and(|count| count >= 3)
    );
    assert!(
        removed_value["summary"]["aliases_removed"]
            .as_array()
            .expect("aliases should be an array")
            .iter()
            .any(|alias| alias.as_str() == Some("fixture"))
    );

    run_repo(
        &service,
        RepoCommand::Status {
            alias: "fixture".to_owned(),
        },
        context("status-after-remove"),
        OutputFormat::Json,
    )
    .await
    .expect_err("removed repository should no longer be registered");

    let reregistered = run_repo(
        &service,
        RepoCommand::Register {
            root_path: repo.path.display().to_string(),
            alias: "fixture".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: Vec::new(),
        },
        context("reregister"),
        OutputFormat::Json,
    )
    .await
    .expect("repository should re-register after removal");
    assert_eq!(
        json_value(&reregistered)["registration"]["alias"],
        "fixture"
    );
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
            reuse_historical: false,
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
                exclude_generated: false,
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

#[tokio::test]
async fn repo_index_worktree_ref_indexes_untracked_worktree_files() {
    let repo = FixtureRepo::create("repo-cli-worktree");
    repo.write(
        "src/lib.rs",
        r#"
pub fn committed_policy() -> u32 {
    1
}
"#,
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    run_repo(
        &service,
        RepoCommand::Register {
            root_path: repo.path.display().to_string(),
            alias: "fixture".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: Vec::new(),
        },
        context("register-worktree"),
        OutputFormat::Json,
    )
    .await
    .expect("repository should register");

    let preview = run_repo(
        &service,
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "worktree".to_owned(),
            dry_run: true,
            reuse_historical: false,
        },
        context("preview-worktree"),
        OutputFormat::Json,
    )
    .await
    .expect("worktree dry-run should preview the HEAD-backed scope");
    let preview_value = json_value(&preview);
    assert_eq!(preview_value["scope"]["requested_ref"], "HEAD");
    assert_eq!(preview_value["preview"]["selected_file_count"], 1);

    run_repo(
        &service,
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "HEAD".to_owned(),
            dry_run: false,
            reuse_historical: false,
        },
        context("index-head"),
        OutputFormat::Json,
    )
    .await
    .expect("base HEAD index should run");

    repo.write(
        "src/generated.rs",
        r#"
pub fn worktree_policy() -> u32 {
    committed_policy()
}
"#,
    );

    let indexed = run_repo(
        &service,
        RepoCommand::Index {
            alias: "fixture".to_owned(),
            ref_selector: "worktree".to_owned(),
            dry_run: false,
            reuse_historical: false,
        },
        context("index-worktree"),
        OutputFormat::Json,
    )
    .await
    .expect("worktree overlay index should run");
    let indexed_value = json_value(&indexed);
    assert_eq!(indexed_value["scope"]["requested_ref"], "worktree");
    assert!(
        indexed_value["scope"]["resolved_commit_sha"]
            .as_str()
            .expect("worktree scope should include a resolved id")
            .starts_with("worktree:")
    );

    let definitions = run_repo(
        &service,
        RepoCommand::Query {
            alias: "fixture".to_owned(),
            query: "worktree_policy".to_owned(),
            kind: CodeQueryKind::Definition,
            limit: 5,
            ref_selector: "worktree".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            freshness: FreshnessPolicy::AllowStale,
            exclude_generated: false,
        },
        context("query-worktree"),
        OutputFormat::Json,
    )
    .await
    .expect("worktree query should read the overlay scope");
    assert_eq!(
        json_value(&definitions)["results"][0]["path"],
        "src/generated.rs"
    );
}
