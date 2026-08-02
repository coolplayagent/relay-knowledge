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
    }
}

#[test]
fn empty_path_set_does_not_create_a_task_seed() {
    let repository = test_repository("test");
    let seed =
        build_incremental_task_seed(&repository, &[], "HEAD", "abc123", "tree1", 0xabc, 1000);

    assert!(seed.is_none());
}

#[test]
fn task_seed_preserves_overlay_identity_and_time() {
    let repository = test_repository("test");
    let paths = vec![PathBuf::from("/tmp/test-watcher/src/main.rs")];
    let seed = build_incremental_task_seed(
        &repository,
        &paths,
        "HEAD",
        "sha123",
        "tree456",
        0xabc,
        1000,
    )
    .expect("seed should be created");

    assert_eq!(seed.repository_id, "repo-test");
    assert_eq!(seed.alias, "test");
    assert_eq!(seed.ref_selector, "HEAD");
    assert_eq!(seed.resolved_commit_sha, "sha123");
    assert_eq!(seed.tree_hash, "tree456");
    assert!(seed.input_fingerprint.starts_with("worktree_overlay:"));
    assert_eq!(seed.now_ms, 1000);
}

#[test]
fn task_seed_payload_is_a_code_index_request() {
    let repository = test_repository("payload");
    let paths = vec![PathBuf::from("/tmp/test-watcher/x.rs")];
    let seed = build_incremental_task_seed(&repository, &paths, "HEAD", "", "", 0xabc, 0)
        .expect("seed should be created");
    let request: crate::domain::CodeIndexRequest =
        serde_json::from_str(&seed.payload_json).expect("payload should decode");

    assert_eq!(request.repository.repository, "payload");
    assert_eq!(request.repository.ref_selector, "HEAD");
    assert_eq!(request.mode, crate::domain::CodeIndexMode::WorktreeOverlay);
}

#[test]
fn task_seed_fingerprint_includes_the_path_set() {
    let repository = test_repository("fingerprint");
    let first_paths = vec![PathBuf::from("/tmp/test-watcher/a.rs")];
    let second_paths = vec![PathBuf::from("/tmp/test-watcher/b.rs")];
    let first =
        build_incremental_task_seed(&repository, &first_paths, "HEAD", "sha", "tree", 0xabc, 0)
            .expect("first seed should be created");
    let second =
        build_incremental_task_seed(&repository, &second_paths, "HEAD", "sha", "tree", 0xabc, 0)
            .expect("second seed should be created");

    assert_ne!(first.input_fingerprint, second.input_fingerprint);
}

#[test]
fn task_seed_fingerprint_includes_content_generation() {
    let repository = test_repository("content-fingerprint");
    let paths = vec![PathBuf::from("/tmp/test-watcher/a.rs")];
    let first = build_incremental_task_seed(&repository, &paths, "HEAD", "sha", "", 0xabc, 0)
        .expect("first seed should be created");
    let second = build_incremental_task_seed(&repository, &paths, "HEAD", "sha", "", 0xdef, 0)
        .expect("second seed should be created");

    assert_ne!(first.tree_hash, second.tree_hash);
    assert_ne!(first.input_fingerprint, second.input_fingerprint);
    assert!(first.payload_json.contains("\"content_fingerprint\""));
}
