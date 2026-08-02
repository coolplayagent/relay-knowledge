use std::collections::BTreeMap;

use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::*;

#[test]
fn submodule_overlap_requires_a_selected_child_scope() {
    let registration = CodeRepositoryRegistration::new(
        "repository-1",
        "fixture",
        "/tmp/fixture",
        vec!["modules/example/src".to_owned()],
        Vec::new(),
    )
    .expect("registration should be valid");
    let selector = CodeRepositorySelector::new(
        "fixture",
        "HEAD",
        vec!["modules/example/src".to_owned()],
        Vec::new(),
    )
    .expect("selector should be valid");
    let previous_hashes =
        BTreeMap::from([("modules/example/src/lib.rs".to_owned(), "hash".to_owned())]);
    let overlay_scope = WorktreeOverlayScope::new(&registration, &selector, &previous_hashes);

    assert!(submodule_path_scope_overlaps(
        "modules/example",
        &overlay_scope
    ));
    assert!(!submodule_path_scope_overlaps(
        "modules/other",
        &overlay_scope
    ));
}
