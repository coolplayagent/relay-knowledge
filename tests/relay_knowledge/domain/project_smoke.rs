use relay_knowledge::{
    KnowledgeEntity, PROJECT_NAME,
    project::{APP_DIR_NAME, DATABASE_FILE_NAME, LINUX_SERVICE_DEFINITION_FILE_NAME},
};

#[test]
fn exposes_project_name() {
    assert_eq!(PROJECT_NAME, "relay-knowledge");
    assert_eq!(APP_DIR_NAME, PROJECT_NAME);
    assert_eq!(DATABASE_FILE_NAME, "relay-knowledge.sqlite");
    assert_eq!(
        LINUX_SERVICE_DEFINITION_FILE_NAME,
        "relay-knowledge.service"
    );
}

#[test]
fn creates_entity_from_owned_strings() {
    let entity = KnowledgeEntity::new(String::from("entity:graph"), String::from("Graph"));

    assert_eq!(entity.id(), "entity:graph");
    assert_eq!(entity.label(), "Graph");
}
