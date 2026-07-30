use super::{GitlinkImpactExpander, changed_gitlink_path_expansion};
use crate::code::{source::gitlink::selector::GitlinkPathSelector, test_fixtures::TempGitRepo};

#[test]
fn ordinary_files_do_not_enter_gitlink_impact_expansion() {
    let repository = TempGitRepo::create("gitlink-impact-regular");
    repository.write("src/lib.rs", "pub fn value() {}\n");
    repository.git(["add", "."]);
    repository.git(["commit", "-m", "regular file"]);
    let head = repository.git_text(["rev-parse", "HEAD"]);
    let mut expander = GitlinkImpactExpander::new(&repository.path, head.clone(), head, 8);

    let paths = expander
        .expanded_paths("src/lib.rs", true, true, &GitlinkPathSelector::all())
        .expect("regular tree lookup");

    assert!(paths.is_none());
}

#[test]
fn stable_gitlinks_resolve_both_sides_without_changed_children() {
    let child = TempGitRepo::create("gitlink-impact-child");
    child.write("src/lib.rs", "pub fn child_value() {}\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);

    let parent = TempGitRepo::create("gitlink-impact-parent");
    parent.write("src/lib.rs", "pub fn parent_value() {}\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    let child_path = child.path.to_string_lossy().into_owned();
    parent.git([
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "--name",
        "impact-child",
        &child_path,
        "vendor/module",
    ]);
    parent.git(["commit", "-am", "add child"]);
    let parent_head = parent.git_text(["rev-parse", "HEAD"]);

    let expansion = changed_gitlink_path_expansion(
        &parent.path,
        "vendor/module",
        &parent_head,
        &parent_head,
        8,
        &GitlinkPathSelector::all(),
    )
    .expect("stable gitlink expansion")
    .expect("path is a gitlink");

    assert!(expansion.base_is_gitlink);
    assert!(expansion.head_is_gitlink);
    assert!(expansion.base_paths.is_empty());
    assert!(expansion.head_paths.is_empty());
}
