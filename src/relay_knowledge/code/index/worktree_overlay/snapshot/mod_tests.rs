// Direct tests for worktree-overlay snapshot assembly.

use std::collections::BTreeMap;

use super::*;

#[test]
fn workspace_entries_remove_deletions_and_replace_changed_byte_counts() {
    let previous_hashes = BTreeMap::from([
        ("src/keep.rs".to_owned(), "keep".to_owned()),
        ("src/remove.rs".to_owned(), "remove".to_owned()),
        ("src/update.rs".to_owned(), "old".to_owned()),
    ]);
    let deleted_paths = vec!["src/remove.rs".to_owned()];
    let files_to_parse = vec![
        ("src/new.rs".to_owned(), vec![1, 2, 3]),
        ("src/update.rs".to_owned(), vec![4, 5]),
    ];

    let entries = workspace_overlay_entries(&previous_hashes, &deleted_paths, &files_to_parse);

    assert_eq!(
        entries
            .into_iter()
            .map(|entry| (entry.path, entry.byte_count))
            .collect::<Vec<_>>(),
        [
            ("src/keep.rs".to_owned(), 0),
            ("src/new.rs".to_owned(), 3),
            ("src/update.rs".to_owned(), 2),
        ]
    );
}
