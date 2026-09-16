use super::*;
use crate::code::test_fixtures::TempGitRepo;

#[test]
fn source_io_status_keeps_rename_copy_unmerged_and_untracked_paths() {
    let changes = parse_changes(
        concat!(
            "1 .M N... 100644 100644 100644 a b src/space name.rs\0",
            "2 R. N... 100644 100644 100644 a b R100 src/new.rs\0src/old.rs\0",
            "2 C. N... 100644 100644 100644 a b C100 src/copied.rs\0src/original.rs\0",
            "u UU N... 100644 100644 100644 100644 a b c src/conflict.rs\0",
            "? src/untracked file.rs\0"
        )
        .as_bytes(),
    )
    .unwrap();
    assert_eq!(changes.len(), 5);
    assert_eq!(changes[0].change.path, "src/space name.rs");
    assert!(!changes[0].change.has_index_change());
    assert!(changes[0].change.has_worktree_change());
    assert_eq!(
        changes[1].change.deleted_source.as_deref(),
        Some("src/old.rs")
    );
    assert!(changes[2].change.deleted_source.is_none());
    assert!(changes[3].known_file);
    assert!(changes[4].change.is_untracked());
    assert!(!changes[4].known_file);
}

#[test]
fn source_io_status_never_infers_file_type_for_directories_or_gitlinks() {
    for (modes, expected) in [
        ("100644 100644 100644", true),
        ("100644 000000 000000", true),
        ("100644 100644 040000", false),
        ("100644 160000 160000", false),
        ("160000 000000 000000", false),
        ("000000 000000 000000", false),
        ("120000 120000 120000", true),
    ] {
        let record = format!("1 .M N... {modes} a b src/path\0");
        assert_eq!(
            parse_changes(record.as_bytes()).unwrap()[0].known_file,
            expected,
            "{modes}"
        );
    }
    for malformed in [
        "unexpected\0",
        "1 .M N...\0",
        "2 R. N... 100644 100644 100644 a b R100 new.rs\0",
    ] {
        assert!(parse_changes(malformed.as_bytes()).is_err());
    }
}

#[test]
fn source_io_status_batch_carries_file_types_for_many_changes() {
    let repo = TempGitRepo::create("typed-submodule-status");
    for index in 0..128 {
        repo.write(&format!("src/{index}.txt"), "before\n");
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "status fixture"]);
    for index in 0..128 {
        repo.write(&format!("src/{index}.txt"), "after\n");
    }
    let changes = read_changes(&repo.path).unwrap();
    assert_eq!(changes.len(), 128);
    assert!(changes.iter().all(|entry| entry.known_file));
    assert!(
        changes
            .iter()
            .all(|entry| entry.change.has_worktree_change())
    );
}
