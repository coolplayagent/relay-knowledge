use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::*;

#[test]
fn previous_gitlink_children_delete_only_selected_unretained_paths() {
    let registration = CodeRepositoryRegistration::new(
        "repository-1",
        "fixture",
        "/tmp/fixture",
        vec!["src/module".to_owned()],
        Vec::new(),
    )
    .expect("registration should be valid");
    let selector =
        CodeRepositorySelector::new("fixture", "HEAD", vec!["src/module".to_owned()], Vec::new())
            .expect("selector should be valid");
    let previous_hashes = BTreeMap::from([
        ("src/module/keep.rs".to_owned(), "keep".to_owned()),
        ("src/module/remove.rs".to_owned(), "remove".to_owned()),
        ("src/other.rs".to_owned(), "other".to_owned()),
    ]);
    let scope = WorktreeOverlayScope::new(&registration, &selector, &previous_hashes);
    let retained_paths = BTreeSet::from(["src/module/keep.rs".to_owned()]);
    let mut hash_input = Vec::new();
    let mut deleted_paths = Vec::new();

    let recorded = record_previous_gitlink_child_deletions(
        "src/module",
        &previous_hashes,
        &scope,
        &retained_paths,
        &mut hash_input,
        &mut deleted_paths,
    )
    .expect("selected previous children should be recorded");

    assert!(recorded);
    assert_eq!(deleted_paths, ["src/module/remove.rs"]);
    assert_eq!(hash_input, b"D\0src/module/remove.rs\0");
}
