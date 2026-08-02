use crate::domain::{GraphVersion, IndexKind, IndexModality};

use super::*;

#[test]
fn task_identity_is_stable_and_scope_delimited() {
    let first = task_id(IndexKind::Bm25, "ab", IndexModality::Text);
    let repeated = task_id(IndexKind::Bm25, "ab", IndexModality::Text);
    let different_scope = task_id(IndexKind::Bm25, "a:b", IndexModality::Text);

    assert_eq!(first, repeated);
    assert_ne!(first, different_scope);
    assert_eq!(first.len(), "index-refresh:".len() + 16);
}

#[test]
fn input_fingerprint_tracks_target_graph_version() {
    let first = input_fingerprint(
        IndexKind::Vector,
        "docs",
        IndexModality::Text,
        GraphVersion::new(1),
    );
    let next = input_fingerprint(
        IndexKind::Vector,
        "docs",
        IndexModality::Text,
        GraphVersion::new(2),
    );

    assert_ne!(first, next);
    assert!(first.starts_with("vector:"));
    assert!(first.ends_with(":text:1"));
}
