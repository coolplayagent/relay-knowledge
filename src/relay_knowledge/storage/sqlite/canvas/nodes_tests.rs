use super::*;

#[test]
fn node_identifiers_preserve_kind_and_full_identity() {
    assert_eq!(entity_node_id("alpha"), "entity:alpha");
    assert_eq!(evidence_node_id("ev"), "evidence:ev");
    assert_eq!(claim_node_id("claim"), "claim:claim");
    assert_eq!(event_node_id("event"), "event:event");
    assert_eq!(scope_node_id("repo"), "scope:repo");
    assert_eq!(
        code_symbol_node_id("repo", "src/lib.rs", "symbol"),
        "code-symbol:repo:src/lib.rs:symbol"
    );
}

#[test]
fn node_details_drop_empty_values_and_labels_are_bounded() {
    let details = detail_map([("present", "value"), ("empty", "")]);
    let label = truncate_label("  first\nsecond third  ", 12);

    assert_eq!(details.len(), 1);
    assert_eq!(details.get("present").map(String::as_str), Some("value"));
    assert_eq!(label, "first secon...");
}

#[test]
fn entity_node_keeps_scope_in_subtitle_and_details() {
    let node = entity_node(
        "entity-id",
        "Entity",
        GraphVersion::new(4),
        Some("docs".to_owned()),
    );

    assert_eq!(node.id, "entity:entity-id");
    assert_eq!(node.subtitle.as_deref(), Some("docs"));
    assert_eq!(
        node.details.get("label").map(String::as_str),
        Some("Entity")
    );
}
