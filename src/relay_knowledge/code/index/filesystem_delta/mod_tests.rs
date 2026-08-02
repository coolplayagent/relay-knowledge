// Direct tests for filesystem delta detection.

use super::{changed_paths_for_filesystem_diff, filesystem_content_hashes_for_paths};
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
