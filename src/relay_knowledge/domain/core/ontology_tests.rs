use super::*;

#[test]
fn typed_identity_is_label_independent_and_scope_aware() {
    let identity = OntologyIdentity::new(
        SourceScope::parse("repo:one").unwrap(),
        "sales",
        "mrr",
        OntologyEntityKind::BusinessTerm,
    )
    .unwrap();
    let same = OntologyIdentity::new(
        SourceScope::parse("repo:one").unwrap(),
        "sales",
        "mrr",
        OntologyEntityKind::BusinessTerm,
    )
    .unwrap();
    let other_domain = OntologyIdentity::new(
        SourceScope::parse("repo:one").unwrap(),
        "support",
        "mrr",
        OntologyEntityKind::BusinessTerm,
    )
    .unwrap();

    assert_eq!(identity.stable_entity_id(), same.stable_entity_id());
    assert_ne!(identity.stable_entity_id(), other_domain.stable_entity_id());
}

#[test]
fn scoped_identity_rejects_untyped_and_oversized_ids() {
    assert!(
        OntologyIdentity::new(
            SourceScope::parse("repo").unwrap(),
            "domain",
            "term",
            OntologyEntityKind::Untyped,
        )
        .is_err()
    );
    assert!(
        OntologyIdentity::new(
            SourceScope::parse("repo").unwrap(),
            "d".repeat(129),
            "term",
            OntologyEntityKind::BusinessTerm,
        )
        .is_err()
    );
}
