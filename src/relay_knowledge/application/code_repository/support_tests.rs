use super::*;

#[test]
fn recognizes_only_default_optional_code_index_lease_unavailable_errors() {
    assert!(storage_error_message_is(
        &StorageError::InvalidInput(CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE.to_owned()),
        CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    ));
    assert!(storage_error_message_is(
        &StorageError::InvalidInput(CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE.to_owned()),
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE,
    ));
    assert!(!storage_error_message_is(
        &StorageError::InvalidInput("code index task lease expired".to_owned()),
        CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    ));
}

#[test]
fn code_index_worker_pid_parses_only_owned_worker_leases() {
    assert_eq!(code_index_worker_pid("code-index-worker-123"), Some(123));
    assert_eq!(code_index_worker_pid("worker-123"), None);
    assert_eq!(code_index_worker_pid("code-index-worker-"), None);
    assert_eq!(code_index_worker_pid("code-index-worker-pid"), None);
}

#[test]
fn current_process_is_treated_as_running() {
    assert!(process_is_running(std::process::id()));
}

#[test]
fn current_fact_version_scope_requires_expected_source_scope() {
    let expected_scope =
        code_snapshot_expected_scope_id("repo", "tree-a", &["src".to_owned()], &[])
            .expect("code snapshots should have a fact-version scope");
    let compatible = status_for_scope(
        Some(expected_scope),
        Some("tree-a"),
        vec!["src".to_owned()],
        Vec::new(),
    );
    let legacy = status_for_scope(
        Some("git_snapshot:0000000000000000".to_owned()),
        Some("tree-a"),
        vec!["src".to_owned()],
        Vec::new(),
    );
    let custom = status_for_scope(
        Some("git_snapshot:legacy".to_owned()),
        Some("tree-a"),
        vec!["src".to_owned()],
        Vec::new(),
    );
    let missing_scope = status_for_scope(None, Some("tree-a"), vec!["src".to_owned()], Vec::new());
    let missing_tree = status_for_scope(
        Some("git_snapshot:0000000000000000".to_owned()),
        None,
        vec!["src".to_owned()],
        Vec::new(),
    );

    assert!(code_scope_matches_current_fact_version(&compatible));
    assert!(!code_scope_matches_current_fact_version(&legacy));
    assert!(code_scope_matches_current_fact_version(&custom));
    assert!(!code_scope_matches_current_fact_version(&missing_scope));
    assert!(!code_scope_matches_current_fact_version(&missing_tree));
}

#[test]
fn degraded_file_count_uses_index_status_reason_shape() {
    let status = CodeRepositoryStatus {
        degraded_reason: Some("25 file(s) degraded during code indexing".to_owned()),
        ..status_for_scope(None, Some("tree-a"), Vec::new(), Vec::new())
    };
    let custom = CodeRepositoryStatus {
        degraded_reason: Some("custom parser warning".to_owned()),
        ..status.clone()
    };

    assert_eq!(degraded_file_count_from_status(&status), Some(25));
    assert_eq!(degraded_file_count_from_status(&custom), None);
}

#[test]
fn active_path_filters_preserve_registration_scope_boundaries() {
    let registration = vec!["src".to_owned()];
    let narrow_task = vec!["src".to_owned(), "src/a.rs".to_owned()];

    assert!(!active_paths_cover_requested_scope(
        &registration,
        &narrow_task,
        &[]
    ));
    assert!(active_paths_cover_requested_scope(
        &registration,
        &narrow_task,
        &["src/a.rs".to_owned()]
    ));
    assert!(active_paths_cover_requested_scope(
        &registration,
        &registration,
        &["src/a.rs".to_owned()]
    ));
    assert!(!active_paths_cover_requested_scope(
        &registration,
        &registration,
        &["tests/a.rs".to_owned()]
    ));
    assert!(!active_paths_cover_requested_scope(
        &[],
        &["src/a.rs".to_owned()],
        &["src".to_owned()]
    ));
}

#[test]
fn active_language_filters_preserve_registration_scope_boundaries() {
    assert!(!active_languages_cover_requested_scope(
        &[],
        &["python".to_owned()],
        &[]
    ));
    assert!(active_languages_cover_requested_scope(
        &[],
        &["python".to_owned()],
        &["python".to_owned()]
    ));
    assert!(!active_languages_cover_requested_scope(
        &["rust".to_owned()],
        &["rust".to_owned()],
        &["python".to_owned()]
    ));
    assert!(!active_languages_cover_requested_scope(
        &[],
        &["python".to_owned()],
        &["rust".to_owned()]
    ));
}

fn status_for_scope(
    source_scope: Option<String>,
    tree_hash: Option<&str>,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters,
        language_filters,
        last_indexed_scope_id: source_scope,
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: tree_hash.map(str::to_owned),
        state: "indexed".to_owned(),
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}
