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

#[test]
fn source_io_submodule_filters_git_proven_files_before_local_metadata() {
    use crate::code::{
        source::local_io::{self, test_fault},
        test_fixtures::TempGitRepo,
    };
    use crate::domain::CodePathIoOperation;

    let child = TempGitRepo::create("submodule-metadata-filter");
    child.write("src/a.rs", "pub fn original() {}\n");
    child.write("src/ignored.txt", "original\n");
    child.git(["add", "."]);
    child.git(["commit", "-m", "metadata fixture"]);
    child.write("src/a.rs", "pub fn updated() {}\n");
    child.write("src/ignored.txt", "updated\n");
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "alias",
        child.path.to_string_lossy(),
        vec!["module/src".into()],
        vec!["rust".into()],
    )
    .unwrap();
    let selector = CodeRepositorySelector::new("alias", "HEAD", vec![], vec![]).unwrap();
    let previous = BTreeMap::from([("module/src/a.rs".into(), "old".into())]);
    let scope = WorktreeOverlayScope::new(&registration, &selector, &previous);
    // The local submodule boundary only needs its parent path and independent Git root.
    let root = child.path.parent().unwrap();
    let relative = child.path.file_name().unwrap().to_str().unwrap();
    for (path, selected) in [("src/ignored.txt", false), ("src/a.rs", true)] {
        let _guard = test_fault::inject(
            child.path.join(path),
            CodePathIoOperation::Metadata,
            0,
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        let mut skipped = Vec::new();
        let mut identity = Vec::new();
        let mut deleted = Vec::new();
        let mut files = Vec::new();
        let mut unchanged = 0;
        let mut recorder = WorktreeOverlayRecorder {
            skipped_paths: &mut skipped,
            scope: &scope,
            previous_hashes: &previous,
            overlay_hash_input: &mut identity,
            deleted_paths: &mut deleted,
            files_to_parse: &mut files,
            skipped_unchanged_count: &mut unchanged,
        };
        assert!(
            record_dirty_submodule_worktree_overlay(root, relative, "module", &mut recorder)
                .unwrap()
        );
        if selected {
            assert_eq!(skipped.len(), 1);
            assert_eq!(skipped[0].path, "module/src/a.rs");
            assert!(files.is_empty());
        } else {
            assert!(skipped.is_empty());
            assert!(deleted.is_empty());
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].0, "module/src/a.rs");
            assert_eq!(
                local_io::symlink_metadata(&child.path.join(path))
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::PermissionDenied,
                "unselected metadata fault must remain unconsumed"
            );
        }
    }
}
