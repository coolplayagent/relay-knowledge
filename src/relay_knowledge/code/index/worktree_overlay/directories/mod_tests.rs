// Direct tests for bounded worktree directory expansion.

use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;

#[test]
fn directory_expansion_is_sorted_and_stops_at_nested_git_metadata() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-overlay-directories-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("incoming/nested")).expect("nested directory should be created");
    fs::create_dir_all(root.join("incoming/vendor/.git"))
        .expect("nested repository marker should be created");
    fs::write(root.join("incoming/z.rs"), b"z").expect("fixture should be written");
    fs::write(root.join("incoming/nested/a.rs"), b"a").expect("fixture should be written");
    fs::write(root.join("incoming/vendor/ignored.rs"), b"ignored")
        .expect("fixture should be written");

    let files = worktree_directory_files(&root, "incoming", &mut Vec::new(), &|_, _| true)
        .expect("directory should be expandable");

    assert_eq!(files, ["incoming/nested/a.rs", "incoming/z.rs"]);
    fs::remove_dir_all(root).expect("fixture should be removed");
}

#[test]
fn source_io_overlay_entry_type_failure_discards_earlier_descendants() {
    use crate::code::{source::local_io::test_fault, test_fixtures::TempSourceDir};
    let source = TempSourceDir::create("overlay-entry-type");
    source.write("incoming/a_nested/a.rs", "pub fn a() {}");
    source.write("incoming/z_blocked/b.rs", "pub fn b() {}");
    let _guard = test_fault::inject_entry_type(
        source.path.join("incoming/z_blocked"),
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    );
    let mut skipped = Vec::new();
    let files =
        worktree_directory_files(&source.path, "incoming", &mut skipped, &|_, _| true).unwrap();
    assert!(files.is_empty());
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].path, "incoming");
    assert_eq!(skipped[0].io.path_kind, CodePathKind::Directory);
    assert_eq!(skipped[0].io.operation, CodePathIoOperation::Metadata);
    assert!(skipped[0].covers("incoming/a_nested/a.rs"));
    assert!(skipped[0].covers("incoming/z_blocked/b.rs"));
}
