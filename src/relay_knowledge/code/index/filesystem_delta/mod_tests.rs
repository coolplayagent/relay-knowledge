// Direct tests for filesystem delta detection.

use super::changed_paths_for_filesystem_diff;
use crate::code::source::filesystem_content_hashes_for_paths;
use crate::code::test_fixtures::TempSourceDir;

#[test]
fn filesystem_diff_reports_deleted_base_paths() {
    let source = TempSourceDir::create("filesystem-diff-deletion");
    source.write("src/lib.rs", "pub fn unchanged() {}\n");
    source.write("src/api.rs", "pub fn removed() {}\n");
    let paths = vec!["src/api.rs".to_owned(), "src/lib.rs".to_owned()];
    let previous_hashes = filesystem_content_hashes_for_paths(&source.path, &paths)
        .expect("base filesystem hashes should compute");
    std::fs::remove_file(source.path.join("src/api.rs")).expect("indexed file should delete");

    let changed_paths =
        changed_paths_for_filesystem_diff(&source.path, "HEAD", &[], &[], &previous_hashes)
            .expect("filesystem diff should compare against stored base hashes");

    assert_eq!(changed_paths, ["src/api.rs".to_owned()]);
}

#[test]
fn source_io_filesystem_delta_excludes_unselected_discovery_directory_diagnostics() {
    use crate::domain::{CodePathIoOperation, CodeRepositoryRegistration, CodeRepositorySelector};
    let source = TempSourceDir::create("filesystem-delta-unselected-io");
    source.write("src/lib.rs", "pub fn retained() {}\n");
    source.write("include/unused.h", "void unused();\n");
    let paths = vec!["src/lib.rs".to_owned()];
    let previous_hashes = filesystem_content_hashes_for_paths(&source.path, &paths).unwrap();
    let registration = CodeRepositoryRegistration {
        repository_id: "repo".into(),
        alias: "fixture".into(),
        root_path: source.path.display().to_string(),
        path_filters: vec!["src".into()],
        language_filters: Vec::new(),
    };
    let selector = CodeRepositorySelector {
        repository: "fixture".into(),
        ref_selector: "HEAD".into(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
    };
    let _fault = crate::code::source::local_io::test_fault::inject(
        source.path.join("include"),
        CodePathIoOperation::ReadDirectory,
        0,
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "discovery directory denied",
        ),
    );
    let delta = super::build_filesystem_delta_snapshot(
        &registration,
        &selector,
        &source.path,
        "HEAD",
        &previous_hashes,
        Some("filesystem:previous"),
        &Default::default(),
    )
    .unwrap();
    assert!(delta.diagnostics.is_empty());
    assert!(delta.deleted_paths.is_empty());
    assert_eq!(delta.skipped_unchanged_count, 1);
}

#[test]
fn source_io_pinned_delta_rejects_a_failure_after_hashing() {
    use crate::domain::{CodePathIoOperation, CodeRepositoryRegistration, CodeRepositorySelector};
    let source = TempSourceDir::create("pinned-delta-io");
    source.write("src/a.rs", "pub fn retained() {}\n");
    let paths = vec!["src/a.rs".to_owned()];
    let previous_hashes = filesystem_content_hashes_for_paths(&source.path, &paths).unwrap();
    let pin = crate::code::source::filesystem_tree_hash_for_paths(&source.path, &paths).unwrap();
    let registration = CodeRepositoryRegistration::new(
        "repo",
        "fixture",
        source.path.to_string_lossy(),
        vec!["src".into()],
        vec![],
    )
    .unwrap();
    let selector = CodeRepositorySelector::new("fixture", &pin, vec![], vec![]).unwrap();
    let _fault = crate::code::source::local_io::test_fault::inject(
        source.path.join("src/a.rs"),
        CodePathIoOperation::Read,
        1,
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied after hashing"),
    );
    let error = super::build_filesystem_delta_snapshot(
        &registration,
        &selector,
        &source.path,
        &pin,
        &previous_hashes,
        Some(&pin),
        &Default::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains(&pin));
    assert!(error.to_string().contains("no longer matches"));
}
