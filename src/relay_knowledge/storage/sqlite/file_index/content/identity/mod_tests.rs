//! Direct content identity stability and scope-separation contracts.

use crate::domain::IndexKind;

use super::*;

#[test]
fn content_identifiers_are_stable_and_scope_aware() {
    let first_entry = entry_key("scope-a", "root-a", "/workspace/readme.md");
    let second_entry = entry_key("scope-a", "root-b", "/workspace/readme.md");

    assert_ne!(first_entry, second_entry);
    assert_eq!(
        chunk_id(&first_entry, 3),
        "file-content-chunk:5b77c4c30882f267:3"
    );
    assert_eq!(
        cursor_key(IndexKind::Bm25, "scope-a", "root-a", "/workspace/readme.md"),
        "bm25\nscope-a\nroot-a\n/workspace/readme.md"
    );
}
