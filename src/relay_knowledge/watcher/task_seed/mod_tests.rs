//! Direct tests for durable watcher task-seed construction.

use std::path::PathBuf;

use super::*;

fn test_repository(alias: &str) -> WatchedRepository {
    WatchedRepository {
        repository_id: format!("repo-{alias}"),
        alias: alias.to_owned(),
        root: PathBuf::from("/tmp/test-watcher"),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        source_scope: format!("scope-{alias}"),
        last_indexed_commit: "commit-base".to_owned(),
    }
}

#[test]
fn commit_seed_pins_refs_and_reuses_one_ref_reconciliation_slot() {
    let repository = test_repository("commit");
    let first = build_commit_task_seed(&repository, "base", "head-one", "tree-one", 10)
        .expect("commit seed");
    let second = build_commit_task_seed(&repository, "head-one", "head-two", "tree-two", 20)
        .expect("next commit seed");
    let request: crate::domain::CodeIndexRequest =
        serde_json::from_str(&first.payload_json).expect("payload should decode");

    assert_eq!(first.input_fingerprint, second.input_fingerprint);
    assert_eq!(first.resolved_commit_sha, "head-one");
    assert_eq!(
        request.mode,
        crate::domain::CodeIndexMode::incremental("base", "head-one").expect("mode")
    );
    assert_eq!(request.repository.ref_selector, "head-one");
}

#[test]
fn commit_seed_skips_unchanged_or_incomplete_identities() {
    let repository = test_repository("commit-skip");
    assert!(build_commit_task_seed(&repository, "same", "same", "tree", 0).is_none());
    assert!(build_commit_task_seed(&repository, "", "head", "tree", 0).is_none());
}

#[test]
fn empty_path_set_does_not_create_a_task_seed() {
    let repository = test_repository("test");
    let seed = build_incremental_task_seed(&repository, &[], 0xabc, 1000);

    assert!(seed.is_none());
}

#[test]
fn task_seed_preserves_overlay_identity_and_time() {
    let repository = test_repository("test");
    let paths = vec![PathBuf::from("/tmp/test-watcher/src/main.rs")];
    let seed = build_incremental_task_seed(&repository, &paths, 0xabc, 1000)
        .expect("seed should be created");

    assert_eq!(seed.repository_id, "repo-test");
    assert_eq!(seed.alias, "test");
    assert_eq!(seed.ref_selector, "commit-base");
    assert_eq!(seed.resolved_commit_sha, "worktree:pending:commit-base");
    assert_eq!(seed.tree_hash, "worktree:pending:commit-base");
    assert_ne!(seed.source_scope, repository.source_scope);
    assert!(seed.input_fingerprint.starts_with("worktree_overlay:"));
    assert_eq!(seed.now_ms, 1000);
}

#[test]
fn task_seed_payload_is_a_code_index_request() {
    let repository = test_repository("payload");
    let paths = vec![PathBuf::from("/tmp/test-watcher/x.rs")];
    let seed =
        build_incremental_task_seed(&repository, &paths, 0xabc, 0).expect("seed should be created");
    let request: crate::domain::CodeIndexRequest =
        serde_json::from_str(&seed.payload_json).expect("payload should decode");

    assert_eq!(request.repository.repository, "payload");
    assert_eq!(request.repository.ref_selector, "commit-base");
    assert_eq!(request.mode, crate::domain::CodeIndexMode::WorktreeOverlay);
}

#[test]
fn task_seed_fingerprint_includes_the_path_set() {
    let repository = test_repository("fingerprint");
    let first_paths = vec![PathBuf::from("/tmp/test-watcher/a.rs")];
    let second_paths = vec![PathBuf::from("/tmp/test-watcher/b.rs")];
    let first = build_incremental_task_seed(&repository, &first_paths, 0xabc, 0)
        .expect("first seed should be created");
    let second = build_incremental_task_seed(&repository, &second_paths, 0xabc, 0)
        .expect("second seed should be created");

    assert_ne!(first.input_fingerprint, second.input_fingerprint);
}

#[test]
fn task_seed_fingerprint_includes_content_generation() {
    let repository = test_repository("content-fingerprint");
    let paths = vec![PathBuf::from("/tmp/test-watcher/a.rs")];
    let first = build_incremental_task_seed(&repository, &paths, 0xabc, 0)
        .expect("first seed should be created");
    let second = build_incremental_task_seed(&repository, &paths, 0xdef, 0)
        .expect("second seed should be created");

    assert_eq!(first.tree_hash, second.tree_hash);
    assert_ne!(first.input_fingerprint, second.input_fingerprint);
    assert!(first.payload_json.contains("\"content_fingerprint\""));
}

#[test]
fn task_seed_uses_clean_base_from_an_active_worktree_identity() {
    let repository = WatchedRepository {
        last_indexed_commit: "worktree:base-commit:overlay-hash".to_owned(),
        ..test_repository("active-overlay")
    };
    let paths = vec![PathBuf::from("/tmp/test-watcher/src/main.rs")];
    let seed = build_incremental_task_seed(&repository, &paths, 0xabc, 10)
        .expect("seed should use clean base");

    assert_eq!(seed.ref_selector, "base-commit");
    assert_eq!(seed.resolved_commit_sha, "worktree:pending:base-commit");
    let request: crate::domain::CodeIndexRequest =
        serde_json::from_str(&seed.payload_json).expect("payload should decode");
    assert_eq!(request.repository.ref_selector, "base-commit");
}

#[test]
fn task_seed_rejects_a_filesystem_snapshot_without_git_commit_semantics() {
    let repository = WatchedRepository {
        last_indexed_commit: "filesystem:base-hash".to_owned(),
        ..test_repository("filesystem")
    };
    let paths = vec![PathBuf::from("/tmp/test-watcher/src/main.rs")];
    assert!(build_incremental_task_seed(&repository, &paths, 0xabc, 10).is_none());
}

#[test]
fn periodic_worktree_reconcile_seed_is_stable_for_one_clean_base() {
    let repository = test_repository("periodic-dirty");
    let first = build_worktree_reconcile_task_seed(&repository, 0xabc, 10).expect("first seed");
    let second = build_worktree_reconcile_task_seed(&repository, 0xabc, 20).expect("second seed");

    assert_eq!(first.input_fingerprint, second.input_fingerprint);
    assert_eq!(first.payload_json, second.payload_json);
    assert_eq!(first.mode, crate::domain::CodeIndexMode::WorktreeOverlay);
    assert_eq!(first.resolved_commit_sha, "worktree:pending:commit-base");
    assert!(first.payload_json.contains("periodic_worktree_reconcile"));
}
