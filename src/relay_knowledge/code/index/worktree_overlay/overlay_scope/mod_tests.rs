// Direct tests for overlay scope and bounded change admission.

use std::collections::BTreeMap;

use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::*;

#[test]
fn bounded_changes_count_only_paths_that_touch_the_selected_scope() {
    let registration = CodeRepositoryRegistration::new(
        "repository-1",
        "fixture",
        "/tmp/fixture",
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should be valid");
    let selector =
        CodeRepositorySelector::new("fixture", "HEAD", vec!["src".to_owned()], Vec::new())
            .expect("selector should be valid");
    let previous_hashes = BTreeMap::from([("src/lib.rs".to_owned(), "hash".to_owned())]);
    let scope = WorktreeOverlayScope::new(&registration, &selector, &previous_hashes);
    let changes = (0..=MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS)
        .map(|index| changes::WorktreePathChange {
            status: " M".to_owned(),
            path: format!("target/generated-{index}.rs"),
            deleted_source: None,
        })
        .collect::<Vec<_>>();

    let bounded =
        bounded_worktree_changes(changes.clone(), &scope).expect("out-of-scope paths stay bounded");

    assert_eq!(bounded, changes);
}
