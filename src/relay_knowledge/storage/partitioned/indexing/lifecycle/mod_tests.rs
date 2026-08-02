use super::super::test_support::partitioned_store;
use super::clear_workspace;

#[tokio::test]
async fn clearing_an_unpublished_workspace_remains_idempotent() {
    let store = partitioned_store("clear-workspace");

    clear_workspace(
        &store,
        "repo-missing".to_owned(),
        "scope-missing".to_owned(),
    )
    .await
    .expect("empty workspace clearing should succeed");
}
