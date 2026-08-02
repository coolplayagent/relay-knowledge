use super::*;

#[test]
fn submodule_child_paths_use_single_normalized_separator() {
    assert_eq!(
        submodule_worktree_parent_path("modules/example/", "src/lib.rs"),
        "modules/example/src/lib.rs"
    );
}
