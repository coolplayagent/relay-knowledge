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
        last_indexed_commit: None,
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
    assert_eq!(watched.path_filters, vec!["src"]);
}
