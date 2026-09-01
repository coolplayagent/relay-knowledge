use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::{
    SOFTWARE_ONTOLOGY_SCHEMA, SoftwareAssertionMode, SoftwareEntity, SoftwareEntityKind,
    SoftwareFactState, SoftwarePredicate, SoftwareSourceKind, SoftwareStatement,
    SoftwareStatementResolution,
};

/// Severity of a software ontology shape diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareShapeSeverity {
    Error,
    Warning,
}

impl SoftwareShapeSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warning),
            _ => None,
        }
    }
}

/// Queryable shape failure modeled after a SHACL validation result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareShapeDiagnostic {
    pub diagnostic_id: String,
    pub shape_id: String,
    pub code: String,
    pub severity: SoftwareShapeSeverity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_key: Option<String>,
    pub field: String,
    pub message: String,
}

/// Validation report kept separate from the software data graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareShapeReport {
    pub conforms: bool,
    pub diagnostics: Vec<SoftwareShapeDiagnostic>,
}

/// Predicate-specific source roles. No global source priority is implied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoftwareAuthorityPolicy {
    pub predicate: SoftwarePredicate,
    pub declared_sources: Vec<SoftwareSourceKind>,
    pub resolved_sources: Vec<SoftwareSourceKind>,
    pub observed_sources: Vec<SoftwareSourceKind>,
}

/// Returns the authority contract for one predicate without choosing a winner.
pub fn software_authority_policy(predicate: SoftwarePredicate) -> SoftwareAuthorityPolicy {
    use SoftwareSourceKind as Source;
    match predicate {
        SoftwarePredicate::DependsOn => SoftwareAuthorityPolicy {
            predicate,
            declared_sources: vec![Source::Manifest],
            resolved_sources: vec![Source::Lockfile, Source::Sbom, Source::BuildAttestation],
            observed_sources: Vec::new(),
        },
        SoftwarePredicate::Builds | SoftwarePredicate::Produces => SoftwareAuthorityPolicy {
            predicate,
            declared_sources: vec![Source::BuildFile, Source::Ci],
            resolved_sources: vec![Source::BuildAttestation],
            observed_sources: vec![Source::BuildAttestation],
        },
        SoftwarePredicate::Deploys | SoftwarePredicate::RunsAs => SoftwareAuthorityPolicy {
            predicate,
            declared_sources: vec![Source::Iac, Source::ServiceDefinition],
            resolved_sources: Vec::new(),
            observed_sources: vec![Source::Runtime, Source::Connector],
        },
        SoftwarePredicate::ProvidesApi | SoftwarePredicate::ConsumesApi => {
            SoftwareAuthorityPolicy {
                predicate,
                declared_sources: vec![Source::ApiSchema, Source::Code],
                resolved_sources: vec![Source::ApiSchema],
                observed_sources: vec![Source::Runtime],
            }
        }
        _ => SoftwareAuthorityPolicy {
            predicate,
            declared_sources: vec![Source::Manifest, Source::Documentation, Source::Code],
            resolved_sources: Vec::new(),
            observed_sources: vec![Source::Runtime, Source::Connector],
        },
    }
}

/// Validates domain/range, evidence, identity, time, and provenance completeness.
pub fn validate_software_shapes(
    entities: &[SoftwareEntity],
    statements: &[SoftwareStatement],
) -> SoftwareShapeReport {
    let mut diagnostics = Vec::new();
    if let Err(error) = SOFTWARE_ONTOLOGY_SCHEMA.validate() {
        diagnostics.push(SoftwareShapeDiagnostic {
            diagnostic_id: diagnostic_id(
                "ontology:SchemaShape",
                "invalid_ontology_schema",
                "software",
                error.field,
            ),
            shape_id: "ontology:SchemaShape".to_owned(),
            code: "invalid_ontology_schema".to_owned(),
            severity: SoftwareShapeSeverity::Error,
            statement_id: None,
            entity_key: None,
            field: error.field.to_owned(),
            message: error.message,
        });
        return SoftwareShapeReport {
            conforms: false,
            diagnostics,
        };
    }
    let entity_kinds = entity_kind_index(entities, &mut diagnostics);
    validate_stable_identities(entities, &mut diagnostics);
    for statement in statements {
        validate_statement(statement, &entity_kinds, &mut diagnostics);
    }
    diagnostics.sort_by(|left, right| left.diagnostic_id.cmp(&right.diagnostic_id));
    SoftwareShapeReport {
        conforms: diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != SoftwareShapeSeverity::Error),
        diagnostics,
    }
}

/// Rejects invalid candidates and marks competing objects as conflicts without overwriting either.
pub fn reconcile_software_statements(
    entities: &[SoftwareEntity],
    mut statements: Vec<SoftwareStatement>,
) -> (Vec<SoftwareStatement>, SoftwareShapeReport) {
    let report = validate_software_shapes(entities, &statements);
    let rejected = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == SoftwareShapeSeverity::Error)
        .filter_map(|diagnostic| diagnostic.statement_id.as_deref())
        .collect::<BTreeSet<_>>();
    for statement in &mut statements {
        if rejected.contains(statement.statement_id.as_str()) {
            statement.fact_state = SoftwareFactState::Rejected;
        } else if statement.resolution_state == SoftwareStatementResolution::Conflicting {
            statement.fact_state = SoftwareFactState::Conflicting;
        }
    }

    let mut competing = BTreeMap::<(String, SoftwarePredicate), BTreeSet<String>>::new();
    for statement in statements
        .iter()
        .filter(|statement| statement.fact_state == SoftwareFactState::Active)
        .filter(|statement| statement.predicate == SoftwarePredicate::Supersedes)
    {
        if let Some(object) = statement.object_identity() {
            competing
                .entry((statement.subject_id.clone(), statement.predicate))
                .or_default()
                .insert(object.to_owned());
        }
    }
    let conflicts = competing
        .into_iter()
        .filter_map(|(key, objects)| (objects.len() > 1).then_some(key))
        .collect::<BTreeSet<_>>();
    for statement in &mut statements {
        if statement.fact_state == SoftwareFactState::Active
            && conflicts.contains(&(statement.subject_id.clone(), statement.predicate))
        {
            statement.fact_state = SoftwareFactState::Conflicting;
            statement.resolution_state = SoftwareStatementResolution::Conflicting;
        }
    }
    (statements, report)
}

#[derive(Debug)]
struct EntityShapeIndex {
    kind: SoftwareEntityKind,
    source_scopes: BTreeSet<String>,
}

fn entity_kind_index(
    entities: &[SoftwareEntity],
    diagnostics: &mut Vec<SoftwareShapeDiagnostic>,
) -> HashMap<String, EntityShapeIndex> {
    let mut kinds = HashMap::new();
    for entity in entities {
        if entity
            .evidence_refs
            .iter()
            .any(|evidence| evidence.source_scope != entity.source_scope)
        {
            diagnostics.push(entity_diagnostic(
                entity,
                "software:ProvenanceShape",
                "cross_scope_entity_evidence",
                "evidence_refs",
                "entity evidence must belong to the entity occurrence source scope",
            ));
        }
        match kinds.entry(entity.entity_key.clone()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(EntityShapeIndex {
                    kind: entity.entity_kind,
                    source_scopes: BTreeSet::from([entity.source_scope.clone()]),
                });
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get().kind != entity.entity_kind {
                    diagnostics.push(entity_diagnostic(
                        entity,
                        "software:StableEntityShape",
                        "entity_kind_conflict",
                        "entity_kind",
                        "one stable entity key cannot identify multiple entity kinds",
                    ));
                }
                entry
                    .get_mut()
                    .source_scopes
                    .insert(entity.source_scope.clone());
            }
        }
    }
    kinds
}

fn validate_stable_identities(
    entities: &[SoftwareEntity],
    diagnostics: &mut Vec<SoftwareShapeDiagnostic>,
) {
    let mut stable =
        BTreeMap::<(String, SoftwareEntityKind, String, Option<String>), String>::new();
    for entity in entities
        .iter()
        .filter(|entity| !entity.entity_kind.is_occurrence_kind())
    {
        let identity = (
            entity.repository_id.clone(),
            entity.entity_kind,
            entity.name.clone(),
            entity.namespace.clone(),
        );
        if let Some(previous) = stable.insert(identity, entity.entity_key.clone())
            && previous != entity.entity_key
        {
            diagnostics.push(entity_diagnostic(
                entity,
                "software:StableEntityShape",
                "unstable_entity_key",
                "entity_key",
                "stable entity identity changed across source scopes",
            ));
        }
    }
}

fn validate_statement(
    statement: &SoftwareStatement,
    entity_kinds: &HashMap<String, EntityShapeIndex>,
    diagnostics: &mut Vec<SoftwareShapeDiagnostic>,
) {
    if statement.evidence_refs.is_empty() {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:ProvenanceShape",
            "missing_evidence",
            "evidence_refs",
            "accepted statements require at least one evidence reference",
        ));
    }
    if statement
        .evidence_refs
        .iter()
        .any(|evidence| evidence.source_scope != statement.source_scope)
    {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:ProvenanceShape",
            "cross_scope_evidence",
            "evidence_refs",
            "statement evidence must belong to the statement source scope",
        ));
    }
    if statement.extractor_id.is_empty() || statement.extractor_version.is_empty() {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:ProvenanceShape",
            "missing_extractor",
            "extractor_version",
            "extractor id and version are required",
        ));
    }
    if statement.confidence_basis_points > 10_000 {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:ConfidenceShape",
            "invalid_confidence",
            "confidence_basis_points",
            "confidence must be between 0 and 10000 basis points",
        ));
    }
    if statement
        .valid_from
        .zip(statement.valid_to)
        .is_some_and(|(from, to)| from > to)
    {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:ValidityShape",
            "invalid_validity_interval",
            "valid_to",
            "valid_to must not precede valid_from",
        ));
    }
    if statement.assertion_mode == SoftwareAssertionMode::Observed
        && statement.observed_at.is_none()
    {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:ObservationShape",
            "missing_observed_at",
            "observed_at",
            "observed statements require observed_at",
        ));
    }
    if statement.object_id.is_some() == statement.object_value.is_some() {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:ObjectShape",
            "invalid_object_cardinality",
            "object_id",
            "exactly one of object_id or object_value is required",
        ));
    } else if statement.object_value.is_some() {
        diagnostics.push(statement_diagnostic(
            statement,
            "ontology:ObjectPropertyShape",
            "literal_object_for_object_property",
            "object_value",
            "software ontology object properties require an ontology entity object",
        ));
    }

    let Some(subject) = entity_kinds.get(&statement.subject_id) else {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:RelationShape",
            "unknown_subject",
            "subject_id",
            "subject_id does not reference an entity in this source scope",
        ));
        return;
    };
    if !subject.source_scopes.contains(&statement.source_scope) {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:RelationShape",
            "cross_scope_subject",
            "subject_id",
            "subject_id has no occurrence in the statement source scope",
        ));
    }
    let schema = &SOFTWARE_ONTOLOGY_SCHEMA;
    if !schema.allows_subject(statement.predicate.as_str(), subject.kind.as_str()) {
        diagnostics.push(statement_diagnostic(
            statement,
            "software:RelationShape",
            "invalid_domain",
            "subject_id",
            "predicate domain does not allow the subject entity kind",
        ));
    }
    if let Some(object_id) = statement.object_id.as_deref() {
        let Some(object) = entity_kinds.get(object_id) else {
            diagnostics.push(statement_diagnostic(
                statement,
                "software:RelationShape",
                "unknown_object",
                "object_id",
                "object_id does not reference an entity in this source scope",
            ));
            return;
        };
        if !object.source_scopes.contains(&statement.source_scope) {
            diagnostics.push(statement_diagnostic(
                statement,
                "software:RelationShape",
                "cross_scope_object",
                "object_id",
                "object_id has no occurrence in the statement source scope",
            ));
        }
        if !schema.allows_relation(
            statement.predicate.as_str(),
            subject.kind.as_str(),
            object.kind.as_str(),
        ) {
            diagnostics.push(statement_diagnostic(
                statement,
                "software:RelationShape",
                "invalid_range",
                "object_id",
                "predicate range does not allow the object entity kind",
            ));
        }
    }
}

fn statement_diagnostic(
    statement: &SoftwareStatement,
    shape_id: &str,
    code: &str,
    field: &str,
    message: &str,
) -> SoftwareShapeDiagnostic {
    SoftwareShapeDiagnostic {
        diagnostic_id: diagnostic_id(shape_id, code, &statement.statement_id, field),
        shape_id: shape_id.to_owned(),
        code: code.to_owned(),
        severity: SoftwareShapeSeverity::Error,
        statement_id: Some(statement.statement_id.clone()),
        entity_key: None,
        field: field.to_owned(),
        message: message.to_owned(),
    }
}

fn entity_diagnostic(
    entity: &SoftwareEntity,
    shape_id: &str,
    code: &str,
    field: &str,
    message: &str,
) -> SoftwareShapeDiagnostic {
    SoftwareShapeDiagnostic {
        diagnostic_id: diagnostic_id(shape_id, code, &entity.entity_key, field),
        shape_id: shape_id.to_owned(),
        code: code.to_owned(),
        severity: SoftwareShapeSeverity::Error,
        statement_id: None,
        entity_key: Some(entity.entity_key.clone()),
        field: field.to_owned(),
        message: message.to_owned(),
    }
}

fn diagnostic_id(shape_id: &str, code: &str, focus: &str, field: &str) -> String {
    super::validation::stable_software_id("software_diagnostic", [shape_id, code, focus, field])
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
