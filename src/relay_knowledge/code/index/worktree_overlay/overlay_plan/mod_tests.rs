// Direct tests for worktree-overlay plan identity.

use super::*;

#[test]
fn plan_identity_distinguishes_clean_and_changed_overlay_inputs() {
    let clean = WorktreeOverlayPlan {
        commit: "abc123".to_owned(),
        changed_path_count: 0,
        path_filters: Vec::new(),
        overlay_hash_input: Vec::new(),
        deleted_paths: Vec::new(),
        files_to_parse: Vec::new(),
        skipped_unchanged_count: 0,
    };
    let clean_identity = clean.identity();
    let changed = WorktreeOverlayPlan {
        overlay_hash_input: b"F\0src/lib.rs\0hash\0".to_vec(),
        ..clean
    };

    let changed_identity = changed.identity();

    assert_ne!(changed_identity, clean_identity);
    assert!(changed_identity.0.starts_with("worktree:abc123:"));
    assert!(changed_identity.1.starts_with("worktree:"));
}
