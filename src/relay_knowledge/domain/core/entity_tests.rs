use super::*;
use crate::domain::SourceScope;

#[test]
fn creates_entity_with_id_and_label() {
    let entity = KnowledgeEntity::new("entity:rust", "Rust");

    assert_eq!(entity.id(), "entity:rust");
    assert_eq!(entity.label(), "Rust");
    assert_eq!(entity.entity_kind(), OntologyEntityKind::Untyped);
    assert!(entity.ontology_identity().is_none());
}

#[test]
fn typed_entity_identity_survives_display_name_changes() {
    let identity = OntologyIdentity::new(
        SourceScope::parse("repo:one").unwrap(),
        "sales",
        "mrr",
        OntologyEntityKind::BusinessTerm,
    )
    .unwrap();
    let first = KnowledgeEntity::from_ontology(identity.clone(), "MRR").unwrap();
    let renamed = KnowledgeEntity::from_ontology(identity, "Monthly Recurring Revenue").unwrap();

    assert_eq!(first.id(), renamed.id());
    assert_eq!(first.entity_kind(), OntologyEntityKind::BusinessTerm);
}
