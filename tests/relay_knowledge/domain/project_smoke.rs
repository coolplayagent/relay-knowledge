use relay_knowledge::{KnowledgeEntity, PROJECT_NAME};

#[test]
fn exposes_project_name() {
    assert_eq!(PROJECT_NAME, "relay-knowledge");
}

#[test]
fn creates_entity_from_owned_strings() {
    let entity = KnowledgeEntity::new(String::from("entity:graph"), String::from("Graph"));

    assert_eq!(entity.id(), "entity:graph");
    assert_eq!(entity.label(), "Graph");
}
