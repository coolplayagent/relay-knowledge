// Direct tests for indexed-scope and filter compatibility.

use super::*;
use crate::domain::code_snapshot_scope_id;

#[test]
fn current_fact_version_scope_requires_expected_source_scope() {
    let expected_scope = code_snapshot_scope_id("repo", "tree-a", &["src".to_owned()], &[]);
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
        &["src/a.rs".to_owned()],
    ));
    assert!(active_paths_cover_requested_scope(
        &registration,
        &registration,
        &["src/a.rs".to_owned()],
    ));
    assert!(!active_paths_cover_requested_scope(
        &registration,
        &registration,
        &["tests/a.rs".to_owned()],
    ));
    assert!(!active_paths_cover_requested_scope(
        &[],
        &["src/a.rs".to_owned()],
        &["src".to_owned()],
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
        &["python".to_owned()],
    ));
    assert!(!active_languages_cover_requested_scope(
        &["rust".to_owned()],
        &["rust".to_owned()],
        &["python".to_owned()],
    ));
    assert!(!active_languages_cover_requested_scope(
        &[],
        &["python".to_owned()],
        &["rust".to_owned()],
    ));
}

#[tokio::test]
async fn pinned_worktree_identity_does_not_reenter_git_ref_resolution() {
    let overlay = "worktree:base:0123456789abcdef";
    let mut status = status_for_scope(
        None,
        Some("worktree:0123456789abcdef"),
        Vec::new(),
        Vec::new(),
    );
    status.last_indexed_commit = Some(overlay.to_owned());
    let selector = CodeRepositorySelector::new("fixture", overlay, Vec::new(), Vec::new())
        .expect("selector should validate");

    let resolved = indexed_commit_for_selector(&status, &selector, overlay.to_owned())
        .await
        .expect("resolved overlay identities should bypass Git");

    assert_eq!(resolved, overlay);
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
