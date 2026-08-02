// Direct tests for worktree change routing.

use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    code::source::changes::WorktreePathChange,
    domain::{CodeRepositoryRegistration, CodeRepositorySelector},
};

use super::*;

#[test]
fn modified_regular_files_flow_into_the_shared_parse_queue() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow the Unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-overlay-change-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("src")).expect("fixture directory should be created");
    let bytes = b"pub fn changed() {}\n";
    fs::write(root.join("src/lib.rs"), bytes).expect("fixture should be written");
    let registration = CodeRepositoryRegistration::new(
        "repository-1",
        "fixture",
        root.to_string_lossy(),
        vec!["src".to_owned()],
        Vec::new(),
    )
    .expect("registration should be valid");
    let selector =
        CodeRepositorySelector::new("fixture", "HEAD", vec!["src".to_owned()], Vec::new())
            .expect("selector should be valid");
    let previous_hashes = BTreeMap::new();
    let overlay_scope = WorktreeOverlayScope::new(&registration, &selector, &previous_hashes);
    let context = WorktreeChangeContext {
        root: &root,
        commit: "base",
        previous_hashes: &previous_hashes,
        overlay_scope: &overlay_scope,
    };
    let change = WorktreePathChange {
        status: " M".to_owned(),
        path: "src/lib.rs".to_owned(),
        deleted_source: None,
    };
    let mut hash_input = Vec::new();
    let mut deleted_paths = Vec::new();
    let mut files_to_parse = Vec::new();
    let mut skipped_unchanged_count = 0;
    let mut outputs = WorktreeFileOutputs {
        overlay_hash_input: &mut hash_input,
        deleted_paths: &mut deleted_paths,
        files_to_parse: &mut files_to_parse,
        skipped_unchanged_count: &mut skipped_unchanged_count,
    };

    record_worktree_change(&context, &change, &mut outputs)
        .expect("selected regular file should be recorded");

    assert_eq!(files_to_parse, [("src/lib.rs".to_owned(), bytes.to_vec())]);
    assert!(deleted_paths.is_empty());
    assert_eq!(skipped_unchanged_count, 0);
    fs::remove_dir_all(root).expect("fixture should be removed");
}
