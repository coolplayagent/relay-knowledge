use super::{
    CODE_SNAPSHOT_FACT_VERSION, append_hash_list, append_hash_part,
    clean_git_commit_from_snapshot_identity, code_snapshot_scope_id,
    code_snapshot_scope_id_with_workspace_detection, code_snapshot_scope_is_fact_versioned,
    code_snapshot_scope_matches_identity, code_snapshot_scope_workspace_semantic, stable_hash64,
};
use crate::domain::{CodeMonorepoWorkspaceFormat, CodeWorkspaceDetectionConfig};

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
fn fact_version_includes_doc_block_owner_anchor_semantics() {
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("doc-block-owner-anchor-v2"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("bounded-type-doc-summary-v1"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("search-owner-v2"));
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("reference-search-groups-v2"));
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
fn fact_versioned_snapshot_scope_requires_generated_hash_shape() {
    assert!(code_snapshot_scope_is_fact_versioned(
        "git_snapshot:0123456789abcdef"
    ));
    assert!(!code_snapshot_scope_is_fact_versioned("git_snapshot:test"));
    assert!(!code_snapshot_scope_is_fact_versioned("manual:test"));
    let base = "git_snapshot:0123456789abcdef";
    assert!(code_snapshot_scope_is_fact_versioned(&format!(
        "{base}:workspace-v1:0"
    )));
    assert!(code_snapshot_scope_is_fact_versioned(&format!(
        "{base}:workspace-v1:7"
    )));
    for malformed in ["00", "01", "8", "-1", "+1", "1x", "1:extra"] {
        assert!(!code_snapshot_scope_is_fact_versioned(&format!(
            "{base}:workspace-v1:{malformed}"
        )));
    }
}

#[test]
fn workspace_scope_semantics_are_canonical_and_backward_compatible() {
    let disabled = CodeWorkspaceDetectionConfig::disabled();
    let ordered = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![
            CodeMonorepoWorkspaceFormat::Pnpm,
            CodeMonorepoWorkspaceFormat::CargoWorkspace,
        ],
    };
    let reordered_duplicate = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: vec![
            CodeMonorepoWorkspaceFormat::CargoWorkspace,
            CodeMonorepoWorkspaceFormat::Pnpm,
            CodeMonorepoWorkspaceFormat::Pnpm,
        ],
    };
    let enabled_empty = CodeWorkspaceDetectionConfig {
        enabled: true,
        supported_formats: Vec::new(),
    };
    let legacy = code_snapshot_scope_id("repo", "tree", &[], &[]);
    let disabled_scope =
        code_snapshot_scope_id_with_workspace_detection("repo", "tree", &[], &[], &disabled);
    let ordered_scope =
        code_snapshot_scope_id_with_workspace_detection("repo", "tree", &[], &[], &ordered);
    let duplicate_scope = code_snapshot_scope_id_with_workspace_detection(
        "repo",
        "tree",
        &[],
        &[],
        &reordered_duplicate,
    );
    let empty_scope =
        code_snapshot_scope_id_with_workspace_detection("repo", "tree", &[], &[], &enabled_empty);

    assert_eq!(legacy, disabled_scope);
    assert_eq!(ordered_scope, duplicate_scope);
    assert_ne!(ordered_scope, legacy);
    assert_ne!(empty_scope, legacy);
    assert!(code_snapshot_scope_matches_identity(
        "repo",
        "tree",
        &[],
        &[],
        &ordered_scope,
    ));
    assert_eq!(
        code_snapshot_scope_workspace_semantic("repo", "tree", &[], &[], &legacy),
        Some(None)
    );
    assert_eq!(
        code_snapshot_scope_workspace_semantic("repo", "tree", &[], &[], &empty_scope),
        Some(Some(0))
    );
}

#[test]
fn canonical_call_read_model_upgrade_invalidates_pre_index_scope_identity() {
    // Persisted scope before canonical-call-selectors-v1 for these same inputs.
    let old_scope = "git_snapshot:a9cee41bf143c3e1";
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("canonical-call-selectors-v1"));
    assert!(!code_snapshot_scope_matches_identity(
        "repo-upgrade",
        "tree-unchanged",
        &[],
        &[],
        old_scope
    ));
    let current = code_snapshot_scope_id("repo-upgrade", "tree-unchanged", &[], &[]);
    assert_ne!(old_scope, current);
    assert!(code_snapshot_scope_matches_identity(
        "repo-upgrade",
        "tree-unchanged",
        &[],
        &[],
        &current
    ));
}

#[test]
fn cpp_declaration_upgrade_invalidates_scopes_with_canonical_call_indexes() {
    assert!(CODE_SNAPSHOT_FACT_VERSION.contains("cpp-callable-declarations-v1"));
    // Same inputs indexed with canonical-call-selectors-v1 before C++ declaration repair.
    let old_scope = "git_snapshot:1747c22227b6262c";
    assert!(!code_snapshot_scope_matches_identity(
        "repo-upgrade",
        "tree-unchanged",
        &[],
        &[],
        old_scope
    ));
}

#[test]
fn python_overload_upgrade_invalidates_the_previous_completed_scope() {
    let previous = CODE_SNAPSHOT_FACT_VERSION.replace(
        "python-overload-declarations-v13",
        "python-overload-declarations-v12",
    );
    assert_ne!(previous, CODE_SNAPSHOT_FACT_VERSION);
    let mut input = Vec::new();
    for value in ["git_snapshot", "repo-upgrade", "tree-unchanged"] {
        append_hash_part(&mut input, value);
    }
    append_hash_list(&mut input, &[]);
    append_hash_list(&mut input, &[]);
    append_hash_part(&mut input, &previous);
    let old_scope = format!("git_snapshot:{:016x}", stable_hash64(&input));
    assert!(!code_snapshot_scope_matches_identity(
        "repo-upgrade",
        "tree-unchanged",
        &[],
        &[],
        &old_scope
    ));
}
