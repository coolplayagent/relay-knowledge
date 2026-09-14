use super::*;
use crate::domain::{SoftwareEntityKind, SoftwarePredicate};

#[test]
fn software_vocabulary_is_a_valid_bounded_rdf_owl_schema() {
    let schema = &SOFTWARE_ONTOLOGY_SCHEMA;

    schema.validate().expect("software ontology schema");
    assert_eq!(schema.version, SOFTWARE_ONTOLOGY_VERSION);
    assert_eq!(schema.namespace_iri, SOFTWARE_ONTOLOGY_NAMESPACE);
    assert_eq!(schema.classes.len(), 21);
    assert_eq!(schema.object_properties.len(), 15);
}

#[test]
fn enum_discriminants_remain_aligned_with_the_versioned_catalog() {
    let entity_kinds = [
        SoftwareEntityKind::Domain,
        SoftwareEntityKind::SoftwareSystem,
        SoftwareEntityKind::Component,
        SoftwareEntityKind::Api,
        SoftwareEntityKind::Resource,
        SoftwareEntityKind::Configuration,
        SoftwareEntityKind::BuildDefinition,
        SoftwareEntityKind::DeploymentUnit,
        SoftwareEntityKind::RuntimeService,
        SoftwareEntityKind::TestCase,
        SoftwareEntityKind::ReleaseArtifact,
        SoftwareEntityKind::PackageComponent,
        SoftwareEntityKind::Sdk,
        SoftwareEntityKind::DocumentationUnit,
        SoftwareEntityKind::Pipeline,
        SoftwareEntityKind::BuildJob,
        SoftwareEntityKind::RepositorySnapshot,
        SoftwareEntityKind::FileRevision,
        SoftwareEntityKind::BuildRun,
        SoftwareEntityKind::DeploymentRevision,
        SoftwareEntityKind::RuntimeObservation,
    ];
    for (kind, definition) in entity_kinds.into_iter().zip(SOFTWARE_CLASSES) {
        assert_eq!(kind.as_str(), definition.id);
        assert_eq!(kind.rdf_local_name(), definition.rdf_local_name);
        assert_eq!(SoftwareEntityKind::parse(definition.id), Some(kind));
        assert_eq!(
            kind.is_occurrence_kind(),
            definition.identity == OntologyClassIdentity::Occurrence
        );
    }

    let predicates = [
        SoftwarePredicate::Contains,
        SoftwarePredicate::ProvidesApi,
        SoftwarePredicate::ConsumesApi,
        SoftwarePredicate::DependsOn,
        SoftwarePredicate::Configures,
        SoftwarePredicate::Builds,
        SoftwarePredicate::Produces,
        SoftwarePredicate::Packages,
        SoftwarePredicate::Deploys,
        SoftwarePredicate::RunsAs,
        SoftwarePredicate::Tests,
        SoftwarePredicate::Documents,
        SoftwarePredicate::DerivedFrom,
        SoftwarePredicate::ObservedAs,
        SoftwarePredicate::Supersedes,
    ];
    for (predicate, definition) in predicates.into_iter().zip(SOFTWARE_PROPERTIES) {
        assert_eq!(predicate.as_str(), definition.id);
        assert_eq!(predicate.rdf_local_name(), definition.rdf_local_name);
        assert_eq!(SoftwarePredicate::parse(definition.id), Some(predicate));
    }
}

#[test]
fn executable_shapes_preserve_pair_specific_and_same_class_rules() {
    let schema = &SOFTWARE_ONTOLOGY_SCHEMA;

    assert!(schema.allows_relation("contains", "domain", "software_system"));
    assert!(!schema.allows_relation("contains", "domain", "component"));
    assert!(schema.allows_relation("supersedes", "deployment_revision", "deployment_revision"));
    assert!(!schema.allows_relation("supersedes", "deployment_revision", "release_artifact"));
    assert!(schema.allows_subject("documents", "documentation_unit"));
    assert!(!schema.allows_relation("documents", "documentation_unit", "documentation_unit"));
    assert!(!schema.allows_relation("derived_from", "missing-a", "missing-b"));
    assert!(!schema.allows_relation("supersedes", "missing", "missing"));
}
