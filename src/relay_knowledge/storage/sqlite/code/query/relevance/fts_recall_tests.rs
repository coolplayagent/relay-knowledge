use super::{leading_hybrid_chunk_recall_anchors, member_access_leaf_terms};

#[test]
fn member_access_recall_keeps_symbol_leaves_but_rejects_source_paths() {
    assert_eq!(
        member_access_leaf_terms("client.connectionState source/lib.rs config.json"),
        ["connectionState"]
    );
}

#[test]
fn leading_recall_anchors_are_lowercase_bounded_and_ordered() {
    let terms = vec![
        "first".to_owned(),
        "Second".to_owned(),
        "third".to_owned(),
        "fourth".to_owned(),
        "fifth".to_owned(),
    ];

    assert_eq!(
        leading_hybrid_chunk_recall_anchors(&terms),
        ["first", "third", "fourth"]
    );
}
