use super::{
    OutputFormat, RepoCommand, run_repo,
    test_support::{FixtureRepo, context, json_value, service_with_memory_store},
};
use crate::domain::{CodeQueryKind, FreshnessPolicy};

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
