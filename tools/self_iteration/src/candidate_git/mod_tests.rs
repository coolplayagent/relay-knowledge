use std::path::PathBuf;

use super::*;

#[test]
fn patch_snapshot_distinguishes_empty_and_non_empty_diffs() {
    let mut snapshot = PatchSnapshot {
        path: PathBuf::from("candidate.patch"),
        diff: " \n".to_owned(),
        sha256: "digest".to_owned(),
        base_ref: "HEAD".to_owned(),
    };

    assert!(!snapshot.has_diff());
    snapshot.diff = "diff --git a/file b/file\n".to_owned();
    assert!(snapshot.has_diff());
}
