use super::*;

#[test]
fn empty_index_is_stale_for_newer_graph_version() {
    let status = IndexStatus::empty(IndexKind::Bm25);

    assert!(status.is_stale_for(GraphVersion::new(1)));
}

#[test]
fn index_kind_has_stable_display_values() {
    assert_eq!(IndexKind::Bm25.to_string(), "bm25");
    assert_eq!(IndexKind::Semantic.to_string(), "semantic");
    assert_eq!(IndexKind::Vector.to_string(), "vector");
}

#[test]
fn index_modality_has_stable_display_values() {
    assert_eq!(IndexModality::Text.to_string(), "text");
    assert_eq!(IndexModality::Image.to_string(), "image");
    assert_eq!(IndexModality::Layout.to_string(), "layout");
    assert_eq!(IndexModality::Table.to_string(), "table");
}

#[test]
fn index_state_has_stable_storage_values() {
    assert_eq!(IndexState::Fresh.as_str(), "fresh");
    assert_eq!(IndexState::Stale.as_str(), "stale");
    assert_eq!(IndexState::Failed.as_str(), "failed");
    assert_eq!(IndexState::Paused.as_str(), "paused");
}
