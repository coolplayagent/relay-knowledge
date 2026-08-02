// Direct tests for regular worktree file recording.

use std::{
    collections::BTreeMap,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;

#[test]
fn status_and_deletion_markers_have_stable_binary_framing() {
    let mut hash_input = Vec::new();
    let mut deleted_paths = Vec::new();

    record_status_marker("src/lib.rs", &mut hash_input);
    record_deleted_path("src/old.rs", &mut hash_input, &mut deleted_paths);
    record_unparseable_path("src/link.rs", &mut hash_input, &mut deleted_paths);

    assert_eq!(
        hash_input,
        b"S\0src/lib.rs\0D\0src/old.rs\0S\0src/link.rs\0D\0src/link.rs\0"
    );
    assert_eq!(deleted_paths, ["src/old.rs", "src/link.rs"]);
}

#[test]
fn file_recording_replaces_deletion_even_when_content_is_known() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-overlay-recording-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("src")).expect("fixture directory should be created");
    let bytes = b"pub fn value() -> u32 { 1 }\n";
    fs::write(root.join("src/lib.rs"), bytes).expect("fixture should be written");
    let previous_hashes = BTreeMap::from([("src/lib.rs".to_owned(), stable_content_hash(bytes))]);
    let mut hash_input = Vec::new();
    let mut deleted_paths = vec!["src/lib.rs".to_owned()];
    let mut files_to_parse = Vec::new();
    let mut skipped_unchanged_count = 0;
    let mut outputs = WorktreeFileOutputs {
        overlay_hash_input: &mut hash_input,
        deleted_paths: &mut deleted_paths,
        files_to_parse: &mut files_to_parse,
        skipped_unchanged_count: &mut skipped_unchanged_count,
    };

    record_file_as(
        &root,
        "src/lib.rs",
        "src/lib.rs",
        &previous_hashes,
        &mut outputs,
    )
    .expect("recreated file should be recorded");

    assert!(deleted_paths.is_empty());
    assert_eq!(files_to_parse, [("src/lib.rs".to_owned(), bytes.to_vec())]);
    assert_eq!(skipped_unchanged_count, 0);
    fs::remove_dir_all(root).expect("fixture should be removed");
}
