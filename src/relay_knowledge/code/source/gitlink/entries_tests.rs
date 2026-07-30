use std::fs;

use super::{
    gitlink_commit_at_tree, submodule_entry_bytes, submodule_path_entries_with_child_filters,
    submodule_root,
};
use crate::code::test_fixtures::TempGitRepo;

#[test]
fn resolves_filtered_entries_and_bytes_from_initialized_submodules() {
    let child = TempGitRepo::create("gitlink-entries-child");
    child.write("src/lib.rs", "pub fn child_value() -> u32 { 7 }\n");
    child.write("docs/readme.md", "# Child\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let child_head = child.git_text(["rev-parse", "HEAD"]);

    let parent = TempGitRepo::create("gitlink-entries-parent");
    parent.write("src/lib.rs", "pub fn parent_value() -> u32 { 1 }\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    let child_path = child.path.to_string_lossy().into_owned();
    parent.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "--name",
        "entries-child",
        &child_path,
        "vendor/module",
    ]);
    parent.git(["commit", "-am", "add child"]);
    let parent_head = parent.git_text(["rev-parse", "HEAD"]);

    assert_eq!(
        gitlink_commit_at_tree(&parent.path, &parent_head, "vendor/module")
            .expect("gitlink lookup"),
        Some(child_head.clone())
    );
    let entries = submodule_path_entries_with_child_filters(
        &parent.path,
        "vendor/module",
        Some(&parent_head),
        &child_head,
        &["src".to_owned()],
    )
    .expect("filtered child entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].parent_path, "vendor/module/src/lib.rs");
    assert_eq!(entries[0].child_path, "src/lib.rs");
    assert_eq!(
        submodule_entry_bytes(&parent.path, "vendor/module", &child_head, "src/lib.rs",)
            .expect("submodule blob"),
        b"pub fn child_value() -> u32 { 7 }\n"
    );
    assert_eq!(
        fs::canonicalize(submodule_root(&parent.path, "vendor/module").expect("submodule root"))
            .expect("canonical submodule root"),
        fs::canonicalize(parent.path.join("vendor/module")).expect("canonical fixture root")
    );
}

#[test]
fn distinguishes_regular_tree_entries_from_gitlinks() {
    let repository = TempGitRepo::create("gitlink-entries-regular");
    repository.write("src/lib.rs", "pub fn value() {}\n");
    repository.git(["add", "."]);
    repository.git(["commit", "-m", "regular file"]);
    let head = repository.git_text(["rev-parse", "HEAD"]);

    assert_eq!(
        gitlink_commit_at_tree(&repository.path, &head, "src/lib.rs").expect("regular tree lookup"),
        None
    );
    assert!(submodule_root(&repository.path, "src/lib.rs").is_err());
}
