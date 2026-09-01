use super::*;

const TEST_CLASSES: [OntologyClassDefinition; 3] = [
    OntologyClassDefinition {
        id: "system",
        rdf_local_name: "System",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "component",
        rdf_local_name: "Component",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "snapshot",
        rdf_local_name: "Snapshot",
        identity: OntologyClassIdentity::Occurrence,
    },
];
const CONTAINS_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&["system"]),
    range: OntologyRangeConstraint::OneOf(&["component"]),
}];
const SUPERSEDES_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::Any,
    range: OntologyRangeConstraint::SameAsSubject,
}];
const DOCUMENTS_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::Any,
    range: OntologyRangeConstraint::DifferentFromSubject,
}];
const TEST_PROPERTIES: [OntologyObjectPropertyDefinition; 3] = [
    OntologyObjectPropertyDefinition {
        id: "contains",
        rdf_local_name: "contains",
        relation_shapes: &CONTAINS_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "supersedes",
        rdf_local_name: "supersedes",
        relation_shapes: &SUPERSEDES_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "documents",
        rdf_local_name: "documents",
        relation_shapes: &DOCUMENTS_SHAPES,
    },
];
const TEST_SCHEMA: OntologySchema = OntologySchema {
    id: "test",
    version: "1.0.0",
    namespace_iri: "https://example.test/ontology#",
    classes: &TEST_CLASSES,
    object_properties: &TEST_PROPERTIES,
};

#[test]
fn ontology_schema_validates_and_executes_relation_shapes() {
    TEST_SCHEMA.validate().expect("valid schema");

    assert!(TEST_SCHEMA.allows_subject("contains", "system"));
    assert!(!TEST_SCHEMA.allows_subject("contains", "component"));
    assert!(TEST_SCHEMA.allows_relation("contains", "system", "component"));
    assert!(!TEST_SCHEMA.allows_relation("contains", "component", "system"));
    assert!(TEST_SCHEMA.allows_relation("supersedes", "snapshot", "snapshot"));
    assert!(!TEST_SCHEMA.allows_relation("supersedes", "snapshot", "system"));
    assert!(TEST_SCHEMA.allows_relation("documents", "system", "component"));
    assert!(!TEST_SCHEMA.allows_relation("documents", "system", "system"));
    assert!(!TEST_SCHEMA.allows_relation("unknown", "system", "component"));
    assert!(!TEST_SCHEMA.allows_subject("documents", "missing"));
    assert!(!TEST_SCHEMA.allows_relation("documents", "missing-a", "missing-b"));
    assert!(!TEST_SCHEMA.allows_relation("supersedes", "missing", "missing"));
}

#[test]
fn ontology_schema_rejects_invalid_identity_namespace_and_rdf_names() {
    let invalid = [
        OntologySchema {
            classes: &[],
            object_properties: &[],
            ..TEST_SCHEMA
        },
        OntologySchema {
            version: "1.0",
            ..TEST_SCHEMA
        },
        OntologySchema {
            namespace_iri: "urn:test",
            ..TEST_SCHEMA
        },
        OntologySchema {
            id: " test ",
            ..TEST_SCHEMA
        },
    ];

    assert!(invalid.iter().all(|schema| schema.validate().is_err()));

    const BAD_CLASSES: [OntologyClassDefinition; 1] = [OntologyClassDefinition {
        id: "system",
        rdf_local_name: "bad:name",
        identity: OntologyClassIdentity::Stable,
    }];
    assert!(
        OntologySchema {
            classes: &BAD_CLASSES,
            object_properties: &[],
            ..TEST_SCHEMA
        }
        .validate()
        .is_err()
    );
}

#[test]
fn ontology_schema_rejects_http_namespaces_without_a_valid_authority() {
    for namespace_iri in [
        "https:///#",
        "https://bad host/#",
        "https:///",
        "https://example.test/ontology",
        "ftp://example.test/ontology#",
    ] {
        assert!(
            OntologySchema {
                namespace_iri,
                ..TEST_SCHEMA
            }
            .validate()
            .is_err(),
            "accepted malformed namespace IRI: {namespace_iri}"
        );
    }
}

#[test]
fn ontology_schema_rejects_duplicates_missing_shapes_and_unknown_classes() {
    const DUPLICATE_CLASSES: [OntologyClassDefinition; 2] = [
        OntologyClassDefinition {
            id: "system",
            rdf_local_name: "System",
            identity: OntologyClassIdentity::Stable,
        },
        OntologyClassDefinition {
            id: "system",
            rdf_local_name: "Other",
            identity: OntologyClassIdentity::Stable,
        },
    ];
    const DUPLICATE_RDF_CLASSES: [OntologyClassDefinition; 2] = [
        OntologyClassDefinition {
            id: "system",
            rdf_local_name: "System",
            identity: OntologyClassIdentity::Stable,
        },
        OntologyClassDefinition {
            id: "other",
            rdf_local_name: "System",
            identity: OntologyClassIdentity::Stable,
        },
    ];
    const EMPTY_PROPERTY: [OntologyObjectPropertyDefinition; 1] =
        [OntologyObjectPropertyDefinition {
            id: "contains",
            rdf_local_name: "contains",
            relation_shapes: &[],
        }];
    const UNKNOWN_DOMAIN_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
        domain: OntologyDomainConstraint::OneOf(&["missing"]),
        range: OntologyRangeConstraint::Any,
    }];
    const UNKNOWN_DOMAIN_PROPERTY: [OntologyObjectPropertyDefinition; 1] =
        [OntologyObjectPropertyDefinition {
            id: "contains",
            rdf_local_name: "contains",
            relation_shapes: &UNKNOWN_DOMAIN_SHAPES,
        }];
    const UNKNOWN_RANGE_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
        domain: OntologyDomainConstraint::Any,
        range: OntologyRangeConstraint::OneOf(&["missing"]),
    }];
    const UNKNOWN_RANGE_PROPERTY: [OntologyObjectPropertyDefinition; 1] =
        [OntologyObjectPropertyDefinition {
            id: "contains",
            rdf_local_name: "contains",
            relation_shapes: &UNKNOWN_RANGE_SHAPES,
        }];

    for (classes, properties) in [
        (DUPLICATE_CLASSES.as_slice(), [].as_slice()),
        (DUPLICATE_RDF_CLASSES.as_slice(), [].as_slice()),
        (TEST_CLASSES.as_slice(), EMPTY_PROPERTY.as_slice()),
        (TEST_CLASSES.as_slice(), UNKNOWN_DOMAIN_PROPERTY.as_slice()),
        (TEST_CLASSES.as_slice(), UNKNOWN_RANGE_PROPERTY.as_slice()),
    ] {
        let schema = OntologySchema {
            classes,
            object_properties: properties,
            ..TEST_SCHEMA
        };
        assert!(schema.validate().is_err());
    }
}

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
