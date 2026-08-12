use super::*;
use crate::code::test_fixtures::TempGitRepo;

#[test]
fn repository_snapshot_resolves_tree_from_the_pinned_commit() {
    let repo = TempGitRepo::create("resolution-pinned-identity");
    repo.write("src/lib.rs", "pub fn pinned() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    repo.git(["tag", "-a", "v1", "-m", "release"]);
    let expected_commit = repo.git_text(["rev-parse", "HEAD"]);
    let expected_tree = repo.git_text(["rev-parse", "HEAD^{tree}"]);

    let resolved = resolve_repository_snapshot(&repo.path, "v1")
        .expect("repository identity should resolve from the pinned tagged commit");

    assert_eq!(resolved, (expected_commit, expected_tree));
}

#[test]
fn scoped_gitlink_filters_normalize_deduplicate_and_honor_root_scope() {
    assert_eq!(
        scoped_gitlink_filters(&[
            "./modules/core/".to_owned(),
            "modules/core".to_owned(),
            "plugins/api".to_owned(),
        ]),
        ["modules/core", "plugins/api"]
    );
    assert!(scoped_gitlink_filters(&["modules/core".to_owned(), ".".to_owned()]).is_empty());
}

#[test]
fn path_ancestors_remain_ordered_from_specific_to_rootward() {
    assert_eq!(
        path_and_ancestors("modules/core/src"),
        ["modules/core/src", "modules/core", "modules"]
    );
    assert!(path_and_ancestors("").is_empty());
}

#[test]
fn gitlink_records_are_identified_by_tree_mode() {
    assert!(git_tree_record_is_gitlink(
        b"160000 commit abc\tvendor/module"
    ));
    assert!(!git_tree_record_is_gitlink(
        b"100644 commit abc\tnot-a-gitlink"
    ));
}

#[test]
fn bounded_gitlink_probe_returns_false_for_a_regular_git_tree() {
    let repo = TempGitRepo::create("resolution-regular-gitlink-probe");
    repo.write("src/lib.rs", "pub fn regular() {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "regular tree"]);
    let commit = repo.git_text(["rev-parse", "HEAD"]);

    assert!(
        !git_tree_has_scoped_gitlinks(&repo.path, &commit, &[])
            .expect("the bounded full-tree probe should complete")
    );
    assert!(
        !git_tree_has_scoped_gitlinks(&repo.path, &commit, &["src".to_owned()])
            .expect("the bounded scoped probe should complete")
    );
}

#[test]
fn bounded_gitlink_probe_detects_full_tree_and_ancestor_overlap() {
    let child = TempGitRepo::create("resolution-gitlink-probe-child");
    child.write("src/lib.rs", "pub fn child() {}\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "child"]);
    let child_commit = child.git_text(["rev-parse", "HEAD"]);
    let parent = TempGitRepo::create("resolution-gitlink-probe-parent");
    parent.write("src/lib.rs", "pub fn parent() {}\n");
    parent.git(["add", "."]);
    parent.git(["commit", "-m", "parent"]);
    parent.git([
        "update-index",
        "--add",
        "--cacheinfo",
        "160000",
        &child_commit,
        "vendor/module",
    ]);
    parent.git(["commit", "-m", "add gitlink"]);
    let commit = parent.git_text(["rev-parse", "HEAD"]);

    assert!(
        git_tree_has_scoped_gitlinks(&parent.path, &commit, &[])
            .expect("the bounded full-tree probe should detect the gitlink")
    );
    assert!(
        git_tree_has_scoped_gitlinks(&parent.path, &commit, &["vendor/module/src".to_owned()],)
            .expect("an exact ancestor probe should detect the gitlink")
    );
    assert!(
        !git_tree_has_scoped_gitlinks(&parent.path, &commit, &["src".to_owned()])
            .expect("an unrelated bounded subtree probe should complete")
    );
}
