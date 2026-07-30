use std::path::Path;

use super::{
    SubmoduleDiffRequest, bounded_submodule_parent_paths, changed_submodule_path_sets,
    parent_submodule_path,
};
use crate::code::source::gitlink::selector::GitlinkPathSelector;

#[test]
fn equal_gitlink_commits_produce_empty_diff_sides_without_repository_access() {
    let paths = changed_submodule_path_sets(
        SubmoduleDiffRequest {
            root: Path::new("/repository-is-not-read"),
            path: "vendor/module",
            git_dir: None,
            base_parent_commit: "parent-base",
            head_parent_commit: "parent-head",
            base_gitlink: "same-commit",
            head_gitlink: "same-commit",
            max_paths: 4,
        },
        &GitlinkPathSelector::all(),
    )
    .expect("equal commits should not access Git")
    .expect("equal commits have a resolved empty diff");

    assert!(paths.base_paths.is_empty());
    assert!(paths.head_paths.is_empty());
}

#[test]
fn missing_child_scope_short_circuits_entry_expansion() {
    let include = |_: &str| false;
    let overlaps = |_: &str| false;
    let child_filters = |_: &str| None;
    let selector = GitlinkPathSelector::new_with_child_filters(&include, &overlaps, &child_filters);

    let paths = bounded_submodule_parent_paths(
        Path::new("/repository-is-not-read"),
        "vendor/module",
        None,
        "parent",
        "child",
        1,
        &selector,
    )
    .expect("missing child scope should avoid Git access");

    assert!(paths.is_empty());
}

#[test]
fn parent_paths_normalize_only_the_join_boundary() {
    assert_eq!(
        parent_submodule_path("vendor/module/", "src/lib.rs"),
        "vendor/module/src/lib.rs"
    );
}
