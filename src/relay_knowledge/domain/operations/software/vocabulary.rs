use crate::domain::core::{
    OntologyClassDefinition, OntologyClassIdentity, OntologyDomainConstraint,
    OntologyObjectPropertyDefinition, OntologyRangeConstraint, OntologyRelationShape,
    OntologySchema,
};

/// Version of the repository software ontology contract exposed on every read model.
pub const SOFTWARE_ONTOLOGY_VERSION: &str = "1.0.0";

/// RDF/OWL namespace shared by the executable schema and JSON-LD exports.
pub const SOFTWARE_ONTOLOGY_NAMESPACE: &str = "https://relay-knowledge.dev/ontology/software/1#";

pub(super) const SOFTWARE_CLASSES: [OntologyClassDefinition; 21] = [
    OntologyClassDefinition {
        id: "domain",
        rdf_local_name: "Domain",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "software_system",
        rdf_local_name: "SoftwareSystem",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "component",
        rdf_local_name: "Component",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "api",
        rdf_local_name: "Api",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "resource",
        rdf_local_name: "Resource",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "configuration",
        rdf_local_name: "Configuration",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "build_definition",
        rdf_local_name: "BuildDefinition",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "deployment_unit",
        rdf_local_name: "DeploymentUnit",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "runtime_service",
        rdf_local_name: "RuntimeService",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "test_case",
        rdf_local_name: "TestCase",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "release_artifact",
        rdf_local_name: "ReleaseArtifact",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "package_component",
        rdf_local_name: "PackageComponent",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "sdk",
        rdf_local_name: "Sdk",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "documentation_unit",
        rdf_local_name: "DocumentationUnit",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "pipeline",
        rdf_local_name: "Pipeline",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "build_job",
        rdf_local_name: "BuildJob",
        identity: OntologyClassIdentity::Stable,
    },
    OntologyClassDefinition {
        id: "repository_snapshot",
        rdf_local_name: "RepositorySnapshot",
        identity: OntologyClassIdentity::Occurrence,
    },
    OntologyClassDefinition {
        id: "file_revision",
        rdf_local_name: "FileRevision",
        identity: OntologyClassIdentity::Occurrence,
    },
    OntologyClassDefinition {
        id: "build_run",
        rdf_local_name: "BuildRun",
        identity: OntologyClassIdentity::Occurrence,
    },
    OntologyClassDefinition {
        id: "deployment_revision",
        rdf_local_name: "DeploymentRevision",
        identity: OntologyClassIdentity::Occurrence,
    },
    OntologyClassDefinition {
        id: "runtime_observation",
        rdf_local_name: "RuntimeObservation",
        identity: OntologyClassIdentity::Occurrence,
    },
];

const CONTAINS_SHAPES: [OntologyRelationShape; 7] = [
    OntologyRelationShape {
        domain: OntologyDomainConstraint::OneOf(&["domain"]),
        range: OntologyRangeConstraint::OneOf(&["software_system"]),
    },
    OntologyRelationShape {
        domain: OntologyDomainConstraint::OneOf(&["software_system"]),
        range: OntologyRangeConstraint::OneOf(&[
            "component",
            "resource",
            "configuration",
            "deployment_unit",
            "documentation_unit",
        ]),
    },
    OntologyRelationShape {
        domain: OntologyDomainConstraint::OneOf(&["component"]),
        range: OntologyRangeConstraint::OneOf(&[
            "component",
            "file_revision",
            "configuration",
            "test_case",
        ]),
    },
    OntologyRelationShape {
        domain: OntologyDomainConstraint::OneOf(&["deployment_unit"]),
        range: OntologyRangeConstraint::OneOf(&["resource"]),
    },
    OntologyRelationShape {
        domain: OntologyDomainConstraint::OneOf(&["release_artifact"]),
        range: OntologyRangeConstraint::OneOf(&["package_component", "component", "file_revision"]),
    },
    OntologyRelationShape {
        domain: OntologyDomainConstraint::OneOf(&["pipeline"]),
        range: OntologyRangeConstraint::OneOf(&["build_job"]),
    },
    OntologyRelationShape {
        domain: OntologyDomainConstraint::OneOf(&["repository_snapshot"]),
        range: OntologyRangeConstraint::OneOf(&[
            "software_system",
            "component",
            "api",
            "resource",
            "configuration",
            "build_definition",
            "deployment_unit",
            "test_case",
            "release_artifact",
            "package_component",
            "sdk",
            "documentation_unit",
            "pipeline",
            "file_revision",
        ]),
    },
];

const PROVIDES_API_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&[
        "software_system",
        "component",
        "runtime_service",
        "sdk",
    ]),
    range: OntologyRangeConstraint::OneOf(&["api"]),
}];
const CONSUMES_API_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&[
        "component",
        "runtime_service",
        "test_case",
        "file_revision",
        "build_definition",
    ]),
    range: OntologyRangeConstraint::OneOf(&["api", "sdk"]),
}];
const DEPENDS_ON_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&[
        "software_system",
        "component",
        "build_definition",
        "deployment_unit",
        "runtime_service",
        "package_component",
        "repository_snapshot",
        "file_revision",
    ]),
    range: OntologyRangeConstraint::OneOf(&[
        "package_component",
        "sdk",
        "component",
        "runtime_service",
        "api",
    ]),
}];
const CONFIGURES_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&["configuration"]),
    range: OntologyRangeConstraint::OneOf(&[
        "build_definition",
        "deployment_unit",
        "runtime_service",
        "component",
        "file_revision",
    ]),
}];
const BUILDS_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&["build_definition", "build_run", "build_job"]),
    range: OntologyRangeConstraint::OneOf(&["release_artifact"]),
}];
const PACKAGES_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&["release_artifact"]),
    range: OntologyRangeConstraint::OneOf(&["package_component", "component", "file_revision"]),
}];
const DEPLOYS_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&["deployment_unit"]),
    range: OntologyRangeConstraint::OneOf(&["release_artifact", "runtime_service"]),
}];
const RUNS_AS_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&["component", "release_artifact", "deployment_unit"]),
    range: OntologyRangeConstraint::OneOf(&["runtime_service"]),
}];
const TESTS_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&["test_case"]),
    range: OntologyRangeConstraint::OneOf(&[
        "component",
        "api",
        "runtime_service",
        "build_definition",
        "release_artifact",
        "file_revision",
        "repository_snapshot",
    ]),
}];
const DOCUMENTS_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::OneOf(&["documentation_unit"]),
    range: OntologyRangeConstraint::DifferentFromSubject,
}];
const UNCONSTRAINED_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::Any,
    range: OntologyRangeConstraint::Any,
}];
const SUPERSEDES_SHAPES: [OntologyRelationShape; 1] = [OntologyRelationShape {
    domain: OntologyDomainConstraint::Any,
    range: OntologyRangeConstraint::SameAsSubject,
}];

pub(super) const SOFTWARE_PROPERTIES: [OntologyObjectPropertyDefinition; 15] = [
    OntologyObjectPropertyDefinition {
        id: "contains",
        rdf_local_name: "contains",
        relation_shapes: &CONTAINS_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "provides_api",
        rdf_local_name: "providesApi",
        relation_shapes: &PROVIDES_API_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "consumes_api",
        rdf_local_name: "consumesApi",
        relation_shapes: &CONSUMES_API_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "depends_on",
        rdf_local_name: "dependsOn",
        relation_shapes: &DEPENDS_ON_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "configures",
        rdf_local_name: "configures",
        relation_shapes: &CONFIGURES_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "builds",
        rdf_local_name: "builds",
        relation_shapes: &BUILDS_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "produces",
        rdf_local_name: "produces",
        relation_shapes: &BUILDS_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "packages",
        rdf_local_name: "packages",
        relation_shapes: &PACKAGES_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "deploys",
        rdf_local_name: "deploys",
        relation_shapes: &DEPLOYS_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "runs_as",
        rdf_local_name: "runsAs",
        relation_shapes: &RUNS_AS_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "tests",
        rdf_local_name: "tests",
        relation_shapes: &TESTS_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "documents",
        rdf_local_name: "documents",
        relation_shapes: &DOCUMENTS_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "derived_from",
        rdf_local_name: "derivedFrom",
        relation_shapes: &UNCONSTRAINED_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "observed_as",
        rdf_local_name: "observedAs",
        relation_shapes: &UNCONSTRAINED_SHAPES,
    },
    OntologyObjectPropertyDefinition {
        id: "supersedes",
        rdf_local_name: "supersedes",
        relation_shapes: &SUPERSEDES_SHAPES,
    },
];

/// Storage-independent executable schema for the software ontology.
pub const SOFTWARE_ONTOLOGY_SCHEMA: OntologySchema = OntologySchema {
    id: "relay_knowledge_software",
    version: SOFTWARE_ONTOLOGY_VERSION,
    namespace_iri: SOFTWARE_ONTOLOGY_NAMESPACE,
    classes: &SOFTWARE_CLASSES,
    object_properties: &SOFTWARE_PROPERTIES,
};

#[cfg(test)]
#[path = "vocabulary_tests.rs"]
mod tests;
