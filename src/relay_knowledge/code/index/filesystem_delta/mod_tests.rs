// Direct tests for filesystem delta detection.

use super::{changed_paths_for_filesystem_diff, filesystem_content_hashes_for_paths};
use crate::code::test_fixtures::TempSourceDir;

#[test]
fn filesystem_provider_add_delete_reparses_unchanged_python_symbols() {
    let source = TempSourceDir::create("filesystem-python-origin");
    source.write(
        "app.py",
        "import typing\n@typing.overload\ndef pick(x:int): ...\ndef pick(x): return x\n",
    );
    let registration = source.registration();
    let selector = source.selector();
    let first = crate::code::index::full_snapshot::build_full_snapshot(
        &registration,
        &selector,
        &source.path,
        &Default::default(),
    )
    .unwrap();
    let hashes = first
        .files
        .iter()
        .map(|file| (file.path.clone(), file.blob_hash.clone()))
        .collect();
    source.write("typing.py", "def overload(f): return f\n");
    let added = super::build_filesystem_delta_snapshot(
        &registration,
        &selector,
        &source.path,
        "HEAD",
        &hashes,
        Some(&first.resolved_commit_sha),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        added
            .symbols
            .iter()
            .filter(|s| s.name == "pick" && s.kind == "function")
            .count(),
        2
    );
    let paths = vec!["app.py".into(), "typing.py".into()];
    let hashes = filesystem_content_hashes_for_paths(&source.path, &paths).unwrap();
    std::fs::remove_file(source.path.join("typing.py")).unwrap();
    let removed = super::build_filesystem_delta_snapshot(
        &registration,
        &selector,
        &source.path,
        "HEAD",
        &hashes,
        Some(&added.resolved_commit_sha),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        removed
            .symbols
            .iter()
            .filter(|s| s.name == "pick" && s.kind == "function_declaration")
            .count(),
        1
    );
    assert!(removed.deleted_paths.iter().any(|path| path == "typing.py"));
}

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
