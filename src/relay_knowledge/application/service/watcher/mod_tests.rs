// Direct tests for repository watcher orchestration.

use super::*;

fn status(last_indexed_scope_id: Option<&str>, stale: bool) -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo-1".to_owned(),
        alias: "core".to_owned(),
        root_path: "/tmp/core".to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: Vec::new(),
        last_indexed_scope_id: last_indexed_scope_id.map(str::to_owned),
        last_indexed_commit: last_indexed_scope_id.map(|_| "commit-1".to_owned()),
        tree_hash: None,
        state: "registered".to_owned(),
        indexed_file_count: 0,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale,
        degraded_reason: None,
    }
}

#[test]
fn watched_repository_from_status_skips_unindexed_repositories() {
    assert!(watched_repository_from_status(&status(None, true)).is_none());
}

#[test]
fn watched_repository_from_status_skips_stale_repositories() {
    assert!(watched_repository_from_status(&status(Some("scope-1"), true)).is_none());
}

#[test]
fn watched_repository_from_status_uses_indexed_scope() {
    let watched =
        watched_repository_from_status(&status(Some("scope-1"), false)).expect("indexed repo");
    assert_eq!(watched.repository_id, "repo-1");
    assert_eq!(watched.alias, "core");
    assert_eq!(watched.source_scope, "scope-1");
    assert_eq!(watched.last_indexed_commit, "commit-1");
    assert_eq!(watched.path_filters, vec!["src"]);
}

#[test]
fn watcher_rejects_existing_dead_letter_as_queued_work() {
    let result = accept_watcher_task(task_record(CodeIndexTaskState::DeadLetter));

    let error = result.expect_err("dead letter must degrade the watcher");
    assert!(error.contains("dead_letter"));
    assert!(error.contains("task-1"));
}

#[test]
fn watcher_accepts_unfinished_durable_work() {
    assert!(accept_watcher_task(task_record(CodeIndexTaskState::Queued)).is_ok());
}

fn task_record(state: CodeIndexTaskState) -> CodeIndexTaskRecord {
    CodeIndexTaskRecord {
        task_id: "task-1".to_owned(),
        repository_id: "repo-1".to_owned(),
        alias: "core".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: "commit-1".to_owned(),
        tree_hash: "tree-1".to_owned(),
        source_scope: "scope-1".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        mode: crate::domain::CodeIndexMode::Full,
        state,
        lease_owner: None,
        lease_expires_at_ms: None,
        attempt_count: 0,
        publication_generation: 0,
        next_retry_at_ms: 0,
        input_fingerprint: "fp-1".to_owned(),
        resource_budget: crate::domain::CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        last_error_kind: Some("index".to_owned()),
        last_error_message: Some("parse failed".to_owned()),
        created_at_ms: 1,
        updated_at_ms: 2,
    }
}
