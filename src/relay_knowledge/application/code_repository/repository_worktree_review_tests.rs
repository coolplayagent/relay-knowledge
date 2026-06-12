use crate::domain::{
    CodeIndexMode, CodeIndexRequest, CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest,
    FreshnessPolicy,
};

use super::repository_test_support::*;

#[tokio::test]
async fn filesystem_worktree_index_is_queryable_with_worktree_ref() {
    let source = FixtureSourceDir::create("filesystem-worktree-queryable");
    source.write(
        "src/lib.rs",
        "pub fn filesystem_worktree_policy() -> u32 { 1 }\n",
    );
    let service = service_with_memory_store().await;

    register_fixture_source(&service, &source, "register-filesystem-worktree-queryable").await;
    service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-filesystem-worktree-base"),
        )
        .await
        .expect("base filesystem index should succeed");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "worktree"),
                mode: CodeIndexMode::WorktreeOverlay,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-filesystem-worktree-overlay"),
        )
        .await
        .expect("filesystem worktree overlay should index");

    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "filesystem_worktree_policy",
                selector("fixture", "worktree"),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-filesystem-worktree-overlay"),
        )
        .await
        .expect("filesystem worktree query should use the indexed filesystem scope");

    assert!(query.results.iter().any(|hit| hit.path == "src/lib.rs"));
}

#[tokio::test]
async fn worktree_overlay_freshness_accepts_query_subset_of_indexed_scope() {
    let repo = FixtureRepo::create("worktree-overlay-query-subset-freshness");
    repo.write(
        "src/selected.rs",
        "pub fn subset_overlay_policy() -> u32 { 1 }\n",
    );
    repo.write("src/other.rs", "pub fn other_overlay_policy() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let service = service_with_memory_store().await;

    register_fixture_repo(&service, &repo, "register-worktree-query-subset").await;
    service
        .index_code_repository(
            request("fixture", "HEAD"),
            context("index-worktree-query-subset-base"),
        )
        .await
        .expect("base index should succeed");
    repo.write(
        "src/selected.rs",
        "pub fn subset_overlay_policy_v2() -> u32 { 2 }\n",
    );
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::WorktreeOverlay,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-worktree-query-subset-overlay"),
        )
        .await
        .expect("unfiltered worktree overlay should index");

    let scoped_worktree = CodeRepositorySelector::new(
        "fixture",
        "worktree",
        vec!["src/selected.rs".to_owned()],
        Vec::new(),
    )
    .expect("selector should validate");
    let query = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "subset_overlay_policy_v2",
                scoped_worktree,
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .expect("query request should validate"),
            context("query-worktree-subset-overlay"),
        )
        .await
        .expect("subset query should use the broader indexed worktree overlay");

    assert!(
        query
            .results
            .iter()
            .any(|hit| hit.path == "src/selected.rs")
    );
}
