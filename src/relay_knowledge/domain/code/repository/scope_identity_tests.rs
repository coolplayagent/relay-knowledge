use super::{
    CODE_SNAPSHOT_FACT_VERSION, clean_git_commit_from_snapshot_identity,
    code_snapshot_expected_scope_id, code_snapshot_scope_id, code_snapshot_scope_is_fact_versioned,
};

#[test]
fn clean_git_commit_parses_clean_and_worktree_identities() {
    assert_eq!(
        clean_git_commit_from_snapshot_identity("abc123"),
        Some("abc123")
    );
    assert_eq!(
        clean_git_commit_from_snapshot_identity("worktree:abc123:overlay456"),
        Some("abc123")
    );
}

#[test]
fn clean_git_commit_rejects_non_git_and_malformed_identities() {
    assert_eq!(
        clean_git_commit_from_snapshot_identity("filesystem:abc123"),
        None
    );
    assert_eq!(
        clean_git_commit_from_snapshot_identity("worktree:abc123"),
        None
    );
    assert_eq!(
        clean_git_commit_from_snapshot_identity("worktree::hash"),
        None
    );
    assert_eq!(clean_git_commit_from_snapshot_identity(""), None);
}

#[test]
fn fact_version_includes_generated_and_web_route_facts() {
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("generated-files-v1"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("web-routes-v1"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("syntax-failure-chunks-v1"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("bounded-config-chunks-v1"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("dense-source-windows-v1"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("c-composite-tags-v1"));
}

#[test]
fn snapshot_scope_id_tracks_tree_and_filters() {
    let scope = code_snapshot_scope_id(
        "repo-1",
        "tree-a",
        &["src".to_owned()],
        &["rust".to_owned()],
    );
    let same = code_snapshot_scope_id(
        "repo-1",
        "tree-a",
        &["src".to_owned()],
        &["rust".to_owned()],
    );
    let different_tree = code_snapshot_scope_id(
        "repo-1",
        "tree-b",
        &["src".to_owned()],
        &["rust".to_owned()],
    );

    assert_eq!(scope, same);
    assert_ne!(scope, different_tree);
    assert!(scope.starts_with("git_snapshot:"));
}

#[test]
fn expected_snapshot_scope_id_is_checked_for_unfiltered_repositories() {
    let scope = code_snapshot_scope_id("repo-1", "tree-a", &[], &[]);
    let expected = code_snapshot_expected_scope_id("repo-1", "tree-a", &[], &[])
        .expect("all repository snapshots should carry a fact version");

    assert_eq!(expected, scope);
}

#[test]
fn fact_versioned_snapshot_scope_requires_generated_hash_shape() {
    assert!(code_snapshot_scope_is_fact_versioned(
        "git_snapshot:0123456789abcdef"
    ));
    assert!(!code_snapshot_scope_is_fact_versioned("git_snapshot:test"));
    assert!(!code_snapshot_scope_is_fact_versioned("manual:test"));
}
