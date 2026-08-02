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

    let files =
        worktree_directory_files(&root, "incoming").expect("directory should be expandable");

    assert_eq!(files, ["incoming/nested/a.rs", "incoming/z.rs"]);
    fs::remove_dir_all(root).expect("fixture should be removed");
}
