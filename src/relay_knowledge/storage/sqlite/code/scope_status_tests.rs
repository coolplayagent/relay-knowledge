use super::code_tests::{
    incremental_snapshot_for_parsed_file, retarget_snapshot_scope, retarget_snapshot_to_fact_scope,
    snapshot_with_chunk,
};
use super::*;
use crate::{domain::CodeRepositoryRegistration, storage::SqliteGraphStore};

#[tokio::test]
async fn incremental_update_rejects_legacy_fact_version_baseline() {
    let store = empty_store_with_repository().await;
    let mut legacy = snapshot_with_chunk("repo", "src/lib.rs", "fn old_policy() {}");
    retarget_snapshot_scope(&mut legacy, "git_snapshot:0000000000000000");
    store
        .apply_code_index_snapshot(legacy)
        .await
        .expect("legacy baseline snapshot should persist");
    let incremental = incremental_snapshot_for_parsed_file();

    let error = store
        .apply_code_index_snapshot(incremental)
        .await
        .expect_err("legacy fact-version baseline should not seed incremental scope");

    assert!(
        error
            .to_string()
            .contains("current base commit and code fact version")
    );
}

#[tokio::test]
async fn scope_status_prefers_active_fact_version_scope_for_duplicate_commit_filters() {
    let store = empty_store_with_repository().await;
    let mut legacy = snapshot_with_chunk("repo", "src/lib.rs", "fn legacy_policy() {}");
    retarget_snapshot_scope(&mut legacy, "git_snapshot:0000000000000000");
    store
        .apply_code_index_snapshot(legacy)
        .await
        .expect("legacy snapshot should persist");
    let mut current = snapshot_with_chunk("repo", "src/lib.rs", "fn current_policy() {}");
    retarget_snapshot_to_fact_scope(&mut current);
    let expected_scope = current.source_scope.clone();
    store
        .apply_code_index_snapshot(current)
        .await
        .expect("current snapshot should persist");

    let status = exact_scope_status(&store).await;

    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some(expected_scope.as_str())
    );
}

#[tokio::test]
async fn scope_status_skips_active_legacy_fact_version_scope_for_duplicate_commit_filters() {
    let store = empty_store_with_repository().await;
    let mut current = snapshot_with_chunk("repo", "src/lib.rs", "fn current_policy() {}");
    retarget_snapshot_to_fact_scope(&mut current);
    let expected_scope = current.source_scope.clone();
    store
        .apply_code_index_snapshot(current)
        .await
        .expect("current snapshot should persist");
    let mut legacy = snapshot_with_chunk("repo", "src/lib.rs", "fn legacy_policy() {}");
    retarget_snapshot_scope(&mut legacy, "git_snapshot:ffffffffffffffff");
    store
        .apply_code_index_snapshot(legacy)
        .await
        .expect("legacy duplicate snapshot should persist");

    let status = exact_scope_status(&store).await;

    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some(expected_scope.as_str())
    );
}

#[tokio::test]
async fn scope_status_rejects_legacy_fact_version_scope_without_current_match() {
    let store = empty_store_with_repository().await;
    let mut legacy = snapshot_with_chunk("repo", "src/lib.rs", "fn legacy_policy() {}");
    retarget_snapshot_scope(&mut legacy, "git_snapshot:ffffffffffffffff");
    store
        .apply_code_index_snapshot(legacy)
        .await
        .expect("legacy snapshot should persist");

    let scoped = store
        .code_repository_scope_status(
            "fixture".to_owned(),
            "commit".to_owned(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("scope status should load");
    let latest = store
        .latest_code_repository_scope_status("fixture".to_owned(), Vec::new(), Vec::new())
        .await
        .expect("latest scope status should load");

    assert!(scoped.is_none());
    assert!(latest.is_none());
}

#[tokio::test]
async fn latest_scope_status_skips_legacy_fact_version_scope_while_scanning() {
    let store = empty_store_with_repository().await;
    let mut legacy = snapshot_with_chunk("repo", "src/lib.rs", "fn legacy_policy() {}");
    retarget_snapshot_scope(&mut legacy, "git_snapshot:ffffffffffffffff");
    store
        .apply_code_index_snapshot(legacy)
        .await
        .expect("legacy snapshot should persist");
    let mut current = snapshot_with_chunk("repo", "src/lib.rs", "fn current_policy() {}");
    retarget_snapshot_to_fact_scope(&mut current);
    let expected_scope = current.source_scope.clone();
    store
        .apply_code_index_snapshot(current)
        .await
        .expect("current snapshot should persist");

    let status = store
        .latest_code_repository_scope_status("fixture".to_owned(), Vec::new(), Vec::new())
        .await
        .expect("latest status should load")
        .expect("current scope should be selected");

    assert_eq!(
        status.last_indexed_scope_id.as_deref(),
        Some(expected_scope.as_str())
    );
}

async fn empty_store_with_repository() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");

    store
}

async fn exact_scope_status(store: &SqliteGraphStore) -> crate::domain::CodeRepositoryStatus {
    store
        .code_repository_scope_status(
            "fixture".to_owned(),
            "commit".to_owned(),
            Vec::new(),
            Vec::new(),
        )
        .await
        .expect("scope status should load")
        .expect("current scope should be selected")
}
