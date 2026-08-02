use super::*;

#[test]
fn name_status_parser_preserves_add_delete_rename_copy_and_type_changes() {
    let changes = parse_name_status_z(
        b"M\0src/lib.rs\0D\0old.rs\0R100\0before.rs\0after.rs\0C100\0a.rs\0b.rs\0T\0link\0",
    )
    .expect("valid name-status records should parse");

    assert_eq!(
        changes,
        [
            GitChange::AddedOrModified {
                path: "src/lib.rs".to_owned(),
            },
            GitChange::Deleted {
                path: "old.rs".to_owned(),
            },
            GitChange::Renamed {
                old_path: "before.rs".to_owned(),
                new_path: "after.rs".to_owned(),
            },
            GitChange::Copied {
                old_path: "a.rs".to_owned(),
                new_path: "b.rs".to_owned(),
            },
            GitChange::TypeChanged {
                path: "link".to_owned(),
            },
        ]
    );
}

#[test]
fn worktree_change_flags_distinguish_index_worktree_and_untracked_states() {
    let staged = WorktreePathChange {
        status: "M ".to_owned(),
        path: "staged.rs".to_owned(),
        deleted_source: None,
    };
    let unstaged = WorktreePathChange {
        status: " M".to_owned(),
        path: "unstaged.rs".to_owned(),
        deleted_source: None,
    };
    let untracked = WorktreePathChange {
        status: "??".to_owned(),
        path: "new.rs".to_owned(),
        deleted_source: None,
    };

    assert!(staged.has_index_change());
    assert!(!staged.has_worktree_change());
    assert!(!unstaged.has_index_change());
    assert!(unstaged.has_worktree_change());
    assert!(untracked.is_untracked());
}
