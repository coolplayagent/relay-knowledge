use std::path::{Path, PathBuf};

use super::{
    push_blob_entry, record_tracked_entries_call, reset_tracked_entries_call_count_for_root,
    tracked_entries_call_count_for_root, tracked_entries_commit_lookup_failed,
    tracked_entries_git_dir_ls_tree_bytes, tracked_entries_ls_tree_bytes,
};
use crate::code::{CodeIndexError, source::changes::TrackedEntryScope};

#[test]
fn push_blob_entry_preserves_prefix_path_and_size() {
    let mut entries = Vec::new();

    push_blob_entry(
        "vendor/lib/",
        "src/lib.rs",
        &["100644", "blob", "abc123", "42"],
        &mut entries,
    );

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "vendor/lib/src/lib.rs");
    assert_eq!(entries[0].byte_count, 42);
}

#[test]
fn tracked_entry_observers_are_isolated_by_repository_root() {
    let first = PathBuf::from("/test/tracked-entries-observer-first");
    let second = PathBuf::from("/test/tracked-entries-observer-second");
    reset_tracked_entries_call_count_for_root(first.clone());
    reset_tracked_entries_call_count_for_root(second.clone());

    record_tracked_entries_call(&first);
    record_tracked_entries_call(&second);
    record_tracked_entries_call(&second);

    assert_eq!(tracked_entries_call_count_for_root(&first), 1);
    assert_eq!(tracked_entries_call_count_for_root(&second), 2);
}

#[test]
fn push_blob_entry_defaults_invalid_size_to_zero() {
    let mut entries = Vec::new();

    push_blob_entry(
        "",
        "src/lib.rs",
        &["100644", "blob", "abc123"],
        &mut entries,
    );

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "src/lib.rs");
    assert_eq!(entries[0].byte_count, 0);
}

#[test]
fn commit_lookup_failure_requires_ls_tree_git_command() {
    let ls_tree = CodeIndexError::Git {
        args: vec!["ls-tree".to_owned(), "missing".to_owned()],
        message: "unknown revision".to_owned(),
    };
    let show = CodeIndexError::Git {
        args: vec!["show".to_owned(), "missing".to_owned()],
        message: "unknown revision".to_owned(),
    };
    let invalid = CodeIndexError::InvalidInput("missing".to_owned());

    assert!(tracked_entries_commit_lookup_failed(&ls_tree));
    assert!(!tracked_entries_commit_lookup_failed(&show));
    assert!(!tracked_entries_commit_lookup_failed(&invalid));
}

#[test]
fn empty_scope_skips_worktree_git_lookup() {
    let bytes = tracked_entries_ls_tree_bytes(
        Path::new("/missing/worktree"),
        "HEAD",
        "",
        &TrackedEntryScope::empty(),
    )
    .expect("empty scope should avoid Git");

    assert!(bytes.is_empty());
}

#[test]
fn empty_scope_skips_git_dir_lookup() {
    let bytes = tracked_entries_git_dir_ls_tree_bytes(
        Path::new("/missing/git-dir"),
        "HEAD",
        "",
        &TrackedEntryScope::empty(),
    )
    .expect("empty scope should avoid Git");

    assert!(bytes.is_empty());
}
