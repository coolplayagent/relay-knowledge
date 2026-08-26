//! Cross-owner code scope-status persistence scenarios.

use std::time::{SystemTime, UNIX_EPOCH};

use super::code_tests::{
    incremental_snapshot_for_parsed_file, retarget_snapshot_scope, retarget_snapshot_to_fact_scope,
    snapshot_with_chunk,
};
use super::*;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexPublicationFence, CodeIndexResourceBudget,
        CodeRepositoryRegistration,
    },
    storage::{
        CodeIndexPublicationTarget, CodeIndexTaskClaimRequest, CodeIndexTaskCompletion,
        CodeIndexTaskSeed, SqliteGraphStore,
    },
};

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

#[tokio::test]
async fn same_tree_commit_alias_keeps_the_previous_commit_queryable() {
    let store = empty_store_with_repository().await;
    let mut first = snapshot_with_chunk("repo", "src/lib.rs", "fn stable_tree() {}");
    retarget_snapshot_to_fact_scope(&mut first);
    first.resolved_commit_sha = "commit-a".to_owned();
    let expected_scope = first.source_scope.clone();
    let tree_hash = first.tree_hash.clone();

    store
        .apply_code_index_snapshot(first)
        .await
        .expect("first commit should persist");
    adopt_same_tree_commit(&store, &expected_scope, &tree_hash, "commit-a", "commit-b").await;

    for commit in ["commit-a", "commit-b"] {
        let status = store
            .code_repository_scope_status(
                "fixture".to_owned(),
                commit.to_owned(),
                Vec::new(),
                Vec::new(),
            )
            .await
            .expect("scope alias should query")
            .expect("commit should resolve to the shared content scope");
        assert_eq!(status.last_indexed_commit.as_deref(), Some(commit));
        assert_eq!(
            status.last_indexed_scope_id.as_deref(),
            Some(expected_scope.as_str())
        );
    }
}

#[tokio::test]
async fn same_tree_commit_alias_remains_a_valid_incremental_base() {
    let store = empty_store_with_repository().await;
    let mut first = snapshot_with_chunk("repo", "src/lib.rs", "fn stable_tree() {}");
    retarget_snapshot_to_fact_scope(&mut first);
    first.resolved_commit_sha = "commit-a".to_owned();
    let source_scope = first.source_scope.clone();
    let tree_hash = first.tree_hash.clone();
    store
        .apply_code_index_snapshot(first)
        .await
        .expect("first commit should persist");
    adopt_same_tree_commit(&store, &source_scope, &tree_hash, "commit-a", "commit-b").await;

    let mut incremental = incremental_snapshot_for_parsed_file();
    incremental.base_resolved_commit_sha = Some("commit-a".to_owned());
    incremental.resolved_commit_sha = "commit-c".to_owned();
    incremental.tree_hash = "tree-c".to_owned();
    retarget_snapshot_to_fact_scope(&mut incremental);

    let summary = store
        .apply_code_index_snapshot(incremental)
        .await
        .expect("older same-tree alias should seed the incremental snapshot");

    assert_eq!(
        summary.base_resolved_commit_sha.as_deref(),
        Some("commit-a")
    );
    assert_eq!(summary.resolved_commit_sha, "commit-c");
}

async fn adopt_same_tree_commit(
    store: &SqliteGraphStore,
    source_scope: &str,
    tree_hash: &str,
    active_commit_sha: &str,
    resolved_commit_sha: &str,
) {
    crate::storage::publish_empty_business_projection_for_test(
        store,
        "repo",
        source_scope,
        active_commit_sha,
    )
    .await
    .expect("active content scope should receive its business projection");
    store
        .refresh_software_global_projection(source_scope.to_owned())
        .await
        .expect("active content scope should receive its software projection");
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_millis() as u64;
    let queued = store
        .queue_code_index_task(CodeIndexTaskSeed {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            ref_selector: resolved_commit_sha.to_owned(),
            resolved_commit_sha: resolved_commit_sha.to_owned(),
            tree_hash: tree_hash.to_owned(),
            source_scope: source_scope.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            mode: CodeIndexMode::Full,
            input_fingerprint: format!("same-tree-alias-{resolved_commit_sha}"),
            resource_budget: CodeIndexResourceBudget::default(),
            payload_json: "{}".to_owned(),
            now_ms,
        })
        .await
        .expect("same-tree alias task should queue");
    let running = store
        .claim_code_index_task(CodeIndexTaskClaimRequest {
            task_id: Some(queued.task_id),
            lease_owner: "same-tree-alias-worker".to_owned(),
            lease_duration_ms: 60_000,
            max_attempts: 3,
            now_ms,
        })
        .await
        .expect("same-tree alias claim should run")
        .expect("same-tree alias task should claim");
    let fence = CodeIndexPublicationFence {
        repository_id: running.repository_id.clone(),
        task_id: running.task_id.clone(),
        lease_owner: "same-tree-alias-worker".to_owned(),
        attempt_count: running.attempt_count,
        generation: running.publication_generation,
    };
    let adopted = store
        .reconcile_code_index_publication_with_fence(
            CodeIndexPublicationTarget {
                task_id: running.task_id.clone(),
                repository_id: running.repository_id.clone(),
                source_scope: running.source_scope.clone(),
                resolved_commit_sha: running.resolved_commit_sha.clone(),
                tree_hash: running.tree_hash.clone(),
                path_filters: running.path_filters.clone(),
                language_filters: running.language_filters.clone(),
            },
            fence,
        )
        .await
        .expect("same-tree alias reconcile should run");
    assert!(adopted, "same-tree content must use alias-only adoption");
    store
        .complete_code_index_task(CodeIndexTaskCompletion {
            task_id: running.task_id,
            lease_owner: "same-tree-alias-worker".to_owned(),
            attempt_count: running.attempt_count,
            publication_generation: running.publication_generation,
            now_ms: now_ms.saturating_add(1),
        })
        .await
        .expect("adopted same-tree alias task should complete");
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
