use super::*;

#[test]
fn creates_entity_with_id_and_label() {
    let entity = KnowledgeEntity::new("entity:rust", "Rust");

    assert_eq!(entity.id(), "entity:rust");
    assert_eq!(entity.label(), "Rust");
}
