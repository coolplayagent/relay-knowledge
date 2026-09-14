use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use super::{DomainError, SourceScope};

const MAX_SCHEMA_CLASSES: usize = 256;
const MAX_SCHEMA_PROPERTIES: usize = 256;
const MAX_SCHEMA_RELATION_SHAPES: usize = 256;
const MAX_SCHEMA_TEXT_BYTES: usize = 512;

/// Whether an ontology class has identity across snapshots or represents one occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyClassIdentity {
    Stable,
    Occurrence,
}

/// One OWL class in a bounded, versioned ontology schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OntologyClassDefinition {
    pub id: &'static str,
    pub rdf_local_name: &'static str,
    pub identity: OntologyClassIdentity,
}

/// A subject-class constraint for an ontology object property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "classes")]
pub enum OntologyDomainConstraint {
    Any,
    OneOf(&'static [&'static str]),
}

/// An object-class constraint evaluated relative to the statement subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "classes")]
pub enum OntologyRangeConstraint {
    Any,
    OneOf(&'static [&'static str]),
    SameAsSubject,
    DifferentFromSubject,
}

/// One SHACL-like domain/range shape attached to an OWL object property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OntologyRelationShape {
    pub domain: OntologyDomainConstraint,
    pub range: OntologyRangeConstraint,
}

/// One RDF/OWL object property and its executable relation shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OntologyObjectPropertyDefinition {
    pub id: &'static str,
    pub rdf_local_name: &'static str,
    pub relation_shapes: &'static [OntologyRelationShape],
}

/// A reusable ontology schema independent from graph storage and repository projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OntologySchema {
    pub id: &'static str,
    pub version: &'static str,
    pub namespace_iri: &'static str,
    pub classes: &'static [OntologyClassDefinition],
    pub object_properties: &'static [OntologyObjectPropertyDefinition],
}

impl OntologySchema {
    /// Validates bounded schema identity, RDF names, uniqueness, and class references.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_schema_text("ontology_schema_id", self.id)?;
        validate_semantic_version(self.version)?;
        validate_namespace_iri(self.namespace_iri)?;
        validate_schema_capacity("ontology_classes", self.classes.len(), MAX_SCHEMA_CLASSES)?;
        validate_schema_capacity(
            "ontology_object_properties",
            self.object_properties.len(),
            MAX_SCHEMA_PROPERTIES,
        )?;
        if self.classes.is_empty() {
            return Err(DomainError::invalid(
                "ontology_classes",
                "must contain at least one ontology class",
            ));
        }

        let mut class_ids = BTreeSet::new();
        let mut class_names = BTreeSet::new();
        for class in self.classes {
            validate_rdf_local_name("ontology_class_id", class.id)?;
            validate_rdf_local_name("ontology_class_rdf_name", class.rdf_local_name)?;
            if !class_ids.insert(class.id) {
                return Err(DomainError::invalid(
                    "ontology_class_id",
                    format!("duplicate ontology class '{}'", class.id),
                ));
            }
            if !class_names.insert(class.rdf_local_name) {
                return Err(DomainError::invalid(
                    "ontology_class_rdf_name",
                    format!("duplicate RDF class name '{}'", class.rdf_local_name),
                ));
            }
        }

        let mut property_ids = BTreeSet::new();
        let mut property_names = BTreeSet::new();
        for property in self.object_properties {
            validate_rdf_local_name("ontology_property_id", property.id)?;
            validate_rdf_local_name("ontology_property_rdf_name", property.rdf_local_name)?;
            validate_schema_capacity(
                "ontology_relation_shapes",
                property.relation_shapes.len(),
                MAX_SCHEMA_RELATION_SHAPES,
            )?;
            if property.relation_shapes.is_empty() {
                return Err(DomainError::invalid(
                    "ontology_relation_shapes",
                    format!(
                        "ontology property '{}' requires at least one shape",
                        property.id
                    ),
                ));
            }
            if !property_ids.insert(property.id) {
                return Err(DomainError::invalid(
                    "ontology_property_id",
                    format!("duplicate ontology property '{}'", property.id),
                ));
            }
            if !property_names.insert(property.rdf_local_name) {
                return Err(DomainError::invalid(
                    "ontology_property_rdf_name",
                    format!("duplicate RDF property name '{}'", property.rdf_local_name),
                ));
            }
            for shape in property.relation_shapes {
                validate_domain_constraint(shape.domain, &class_ids)?;
                validate_range_constraint(shape.range, &class_ids)?;
            }
        }
        Ok(())
    }

    /// Returns whether a property admits the given subject class.
    pub fn allows_subject(&self, property_id: &str, subject_class_id: &str) -> bool {
        if !self
            .classes
            .iter()
            .any(|class| class.id == subject_class_id)
        {
            return false;
        }
        self.object_properties
            .iter()
            .find(|property| property.id == property_id)
            .is_some_and(|property| {
                property
                    .relation_shapes
                    .iter()
                    .any(|shape| domain_matches(shape.domain, subject_class_id))
            })
    }

    /// Returns whether an RDF object relation conforms to a declared domain/range shape.
    pub fn allows_relation(
        &self,
        property_id: &str,
        subject_class_id: &str,
        object_class_id: &str,
    ) -> bool {
        if !self
            .classes
            .iter()
            .any(|class| class.id == subject_class_id)
            || !self.classes.iter().any(|class| class.id == object_class_id)
        {
            return false;
        }
        self.object_properties
            .iter()
            .find(|property| property.id == property_id)
            .is_some_and(|property| {
                property.relation_shapes.iter().any(|shape| {
                    domain_matches(shape.domain, subject_class_id)
                        && range_matches(shape.range, subject_class_id, object_class_id)
                })
            })
    }
}

fn validate_schema_capacity(
    field: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), DomainError> {
    if actual > maximum {
        return Err(DomainError::invalid(
            field,
            format!("must contain {maximum} entries or fewer"),
        ));
    }
    Ok(())
}

fn validate_schema_text(field: &'static str, value: &str) -> Result<(), DomainError> {
    if value.is_empty() {
        return Err(DomainError::invalid(field, "must not be empty"));
    }
    if value.len() > MAX_SCHEMA_TEXT_BYTES {
        return Err(DomainError::invalid(
            field,
            format!("must be {MAX_SCHEMA_TEXT_BYTES} bytes or less"),
        ));
    }
    if value.trim() != value || value.contains('\0') {
        return Err(DomainError::invalid(
            field,
            "must be trimmed and contain no NUL bytes",
        ));
    }
    Ok(())
}

fn validate_semantic_version(version: &str) -> Result<(), DomainError> {
    validate_schema_text("ontology_schema_version", version)?;
    if version.split('.').count() != 3
        || version
            .split('.')
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(DomainError::invalid(
            "ontology_schema_version",
            "must be a numeric major.minor.patch version",
        ));
    }
    Ok(())
}

fn validate_namespace_iri(namespace_iri: &str) -> Result<(), DomainError> {
    validate_schema_text("ontology_namespace_iri", namespace_iri)?;
    let parsed = Url::parse(namespace_iri).map_err(|_| {
        DomainError::invalid(
            "ontology_namespace_iri",
            "must be an absolute HTTP(S) IRI with a host and ending in '#' or '/'",
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !(namespace_iri.ends_with('#') || namespace_iri.ends_with('/'))
    {
        return Err(DomainError::invalid(
            "ontology_namespace_iri",
            "must be an absolute HTTP(S) IRI with a host and ending in '#' or '/'",
        ));
    }
    Ok(())
}

fn validate_rdf_local_name(field: &'static str, value: &str) -> Result<(), DomainError> {
    validate_schema_text(field, value)?;
    let mut bytes = value.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(DomainError::invalid(
            field,
            "must be a bounded RDF local name",
        ));
    }
    Ok(())
}

fn validate_domain_constraint(
    constraint: OntologyDomainConstraint,
    class_ids: &BTreeSet<&str>,
) -> Result<(), DomainError> {
    if let OntologyDomainConstraint::OneOf(classes) = constraint {
        validate_class_list("ontology_property_domain", classes, class_ids)?;
    }
    Ok(())
}

fn validate_range_constraint(
    constraint: OntologyRangeConstraint,
    class_ids: &BTreeSet<&str>,
) -> Result<(), DomainError> {
    if let OntologyRangeConstraint::OneOf(classes) = constraint {
        validate_class_list("ontology_property_range", classes, class_ids)?;
    }
    Ok(())
}

fn validate_class_list(
    field: &'static str,
    classes: &[&str],
    class_ids: &BTreeSet<&str>,
) -> Result<(), DomainError> {
    if classes.is_empty() {
        return Err(DomainError::invalid(field, "must not be empty"));
    }
    validate_schema_capacity(field, classes.len(), MAX_SCHEMA_CLASSES)?;
    let mut unique = BTreeSet::new();
    for class in classes {
        if !class_ids.contains(class) {
            return Err(DomainError::invalid(
                field,
                format!("references unknown ontology class '{class}'"),
            ));
        }
        if !unique.insert(*class) {
            return Err(DomainError::invalid(
                field,
                format!("contains duplicate ontology class '{class}'"),
            ));
        }
    }
    Ok(())
}

fn domain_matches(constraint: OntologyDomainConstraint, subject_class_id: &str) -> bool {
    match constraint {
        OntologyDomainConstraint::Any => true,
        OntologyDomainConstraint::OneOf(classes) => classes.contains(&subject_class_id),
    }
}

fn range_matches(
    constraint: OntologyRangeConstraint,
    subject_class_id: &str,
    object_class_id: &str,
) -> bool {
    match constraint {
        OntologyRangeConstraint::Any => true,
        OntologyRangeConstraint::OneOf(classes) => classes.contains(&object_class_id),
        OntologyRangeConstraint::SameAsSubject => subject_class_id == object_class_id,
        OntologyRangeConstraint::DifferentFromSubject => subject_class_id != object_class_id,
    }
}

/// Typed ontology node identity. Untyped legacy graph entities keep their label-derived ids.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyEntityKind {
    #[default]
    Untyped,
    BusinessDomain,
    BusinessTerm,
}

impl OntologyEntityKind {
    /// Stable storage and wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Untyped => "untyped",
            Self::BusinessDomain => "business_domain",
            Self::BusinessTerm => "business_term",
        }
    }
}

/// Immutable ontology identity independent of a display label.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OntologyIdentity {
    pub source_scope: SourceScope,
    pub domain_id: String,
    pub entity_id: String,
    pub entity_kind: OntologyEntityKind,
}

impl OntologyIdentity {
    /// Validates the scoped identity used to create stable typed entity ids.
    pub fn new(
        source_scope: SourceScope,
        domain_id: impl Into<String>,
        entity_id: impl Into<String>,
        entity_kind: OntologyEntityKind,
    ) -> Result<Self, DomainError> {
        let domain_id = validate_identity_text("domain_id", domain_id.into())?;
        let entity_id = validate_identity_text("entity_id", entity_id.into())?;
        if entity_kind == OntologyEntityKind::Untyped {
            return Err(DomainError::invalid(
                "entity_kind",
                "scoped ontology identities must be typed",
            ));
        }
        Ok(Self {
            source_scope,
            domain_id,
            entity_id,
            entity_kind,
        })
    }

    /// Returns a deterministic id that does not depend on the display name.
    pub fn stable_entity_id(&self) -> String {
        let mut digest = Sha256::new();
        for part in [
            self.source_scope.as_str(),
            self.domain_id.as_str(),
            self.entity_id.as_str(),
            self.entity_kind.as_str(),
        ] {
            digest.update((part.len() as u64).to_be_bytes());
            digest.update(part.as_bytes());
        }
        format!("ontology:{:x}", digest.finalize())
    }
}

fn validate_identity_text(field: &'static str, value: String) -> Result<String, DomainError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(DomainError::invalid(field, "must not be empty"));
    }
    if value.len() > 128 {
        return Err(DomainError::invalid(field, "must be 128 bytes or less"));
    }
    if value.contains('\0') {
        return Err(DomainError::invalid(field, "must not contain NUL bytes"));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
#[path = "ontology_tests.rs"]
mod tests;
