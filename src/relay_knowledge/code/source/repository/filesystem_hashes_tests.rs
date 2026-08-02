use std::collections::BTreeMap;

use super::*;

#[test]
fn filesystem_tree_hash_tracks_path_and_content_identity() {
    let first = BTreeMap::from([("src/lib.rs".to_owned(), "one".to_owned())]);
    let second = BTreeMap::from([("src/lib.rs".to_owned(), "two".to_owned())]);

    let first_hash = filesystem_tree_hash_from_path_hashes(&first);

    assert!(source_commit_is_filesystem(&first_hash));
    assert_ne!(first_hash, filesystem_tree_hash_from_path_hashes(&second));
}

#[test]
fn filesystem_blob_verification_rejects_changed_content() {
    let paths = vec!["src/lib.rs".to_owned()];
    let expected = BTreeMap::from([("src/lib.rs".to_owned(), stable_content_hash(b"expected"))]);

    let error = ensure_filesystem_blobs_match_content_hashes(
        "filesystem:planned",
        &paths,
        &[b"changed".to_vec()],
        &expected,
    )
    .expect_err("changed bytes must be rejected");

    assert!(error.to_string().contains("src/lib.rs"));
}
