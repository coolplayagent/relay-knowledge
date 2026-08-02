use super::*;

#[test]
fn git_tree_hash_preserves_plain_tree_and_tracks_submodule_state() {
    let parent = "parent-tree";

    assert_eq!(git_tree_hash_with_submodules(parent, &[]), parent);
    assert_ne!(
        git_tree_hash_with_submodules(parent, &["vendor/sdk:abc123".to_owned()]),
        parent
    );
}
