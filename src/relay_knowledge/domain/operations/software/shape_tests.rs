use std::collections::BTreeMap;

use super::*;
use crate::domain::{GraphVersion, RepositoryCodeRange, SoftwareEntityInput, SoftwareEvidenceRef};

fn entity(kind: SoftwareEntityKind, name: &str) -> SoftwareEntity {
    SoftwareEntity::new(SoftwareEntityInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        entity_kind: kind,
        name: name.to_owned(),
        namespace: None,
        source_kind: SoftwareSourceKind::Code,
        evidence_refs: vec![evidence("scope")],
        attributes: BTreeMap::new(),
        created_graph_version: GraphVersion::new(1),
    })
    .expect("entity")
}

fn evidence(scope: &str) -> SoftwareEvidenceRef {
    SoftwareEvidenceRef::new(
        scope,
        "src/lib.rs",
        RepositoryCodeRange { start: 1, end: 1 },
    )
    .expect("evidence")
}

fn statement(subject: &SoftwareEntity, object: &SoftwareEntity) -> SoftwareStatement {
    SoftwareStatement::candidate(crate::domain::SoftwareStatementInput {
        subject_id: subject.entity_key.clone(),
        predicate: SoftwarePredicate::ProvidesApi,
        object_id: Some(object.entity_key.clone()),
        object_value: None,
        source_scope: "scope".to_owned(),
        source_kind: SoftwareSourceKind::Code,
        evidence_refs: vec![evidence("scope")],
        assertion_mode: SoftwareAssertionMode::Extracted,
        resolution_state: SoftwareStatementResolution::Resolved,
        valid_from: None,
        valid_to: None,
        observed_at: None,
        extractor_id: "relay-knowledge/software-ontology".to_owned(),
        extractor_version: "1".to_owned(),
        confidence_basis_points: 9_000,
        fact_state: SoftwareFactState::Active,
    })
}

#[test]
fn shape_validator_reports_domain_range_provenance_and_validity_failures() {
    let subject = entity(SoftwareEntityKind::TestCase, "smoke");
    let object = entity(SoftwareEntityKind::Resource, "database");
    let mut invalid = statement(&subject, &object);
    invalid.evidence_refs.clear();
    invalid.valid_from = Some(20);
    invalid.valid_to = Some(10);
    invalid.extractor_version.clear();

    let report = validate_software_shapes(&[subject, object], &[invalid]);
    let codes = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<BTreeSet<_>>();

    assert!(!report.conforms);
    assert!(codes.contains("missing_evidence"));
    assert!(codes.contains("missing_extractor"));
    assert!(codes.contains("invalid_validity_interval"));
    assert!(codes.contains("invalid_domain"));
    assert!(codes.contains("invalid_range"));
}

#[test]
fn conflicting_objects_remain_as_queryable_competing_statements() {
    let deployment = entity(SoftwareEntityKind::DeploymentRevision, "current");
    let first = entity(SoftwareEntityKind::DeploymentRevision, "previous-a");
    let second = entity(SoftwareEntityKind::DeploymentRevision, "previous-b");
    let mut first_statement = statement(&deployment, &first);
    first_statement.predicate = SoftwarePredicate::Supersedes;
    let mut second_statement = statement(&deployment, &second);
    second_statement.predicate = SoftwarePredicate::Supersedes;
    second_statement.statement_id.push_str("-second");

    let (statements, report) = reconcile_software_statements(
        &[deployment, first, second],
        vec![first_statement, second_statement],
    );

    assert!(report.conforms);
    assert!(statements.iter().all(|statement| {
        statement.fact_state == SoftwareFactState::Conflicting
            && statement.resolution_state == SoftwareStatementResolution::Conflicting
    }));
}

#[test]
fn plural_build_outputs_are_not_misclassified_as_conflicts() {
    let build = entity(SoftwareEntityKind::BuildDefinition, "package");
    let first = entity(SoftwareEntityKind::ReleaseArtifact, "relay-a");
    let second = entity(SoftwareEntityKind::ReleaseArtifact, "relay-b");
    let mut first_statement = statement(&build, &first);
    first_statement.predicate = SoftwarePredicate::Produces;
    let mut second_statement = statement(&build, &second);
    second_statement.predicate = SoftwarePredicate::Produces;
    second_statement.statement_id.push_str("-second");

    let (statements, report) = reconcile_software_statements(
        &[build, first, second],
        vec![first_statement, second_statement],
    );

    assert!(report.conforms);
    assert!(
        statements
            .iter()
            .all(|statement| statement.fact_state == SoftwareFactState::Active)
    );
}

#[test]
fn cross_scope_evidence_is_rejected_instead_of_accepted() {
    let component = entity(SoftwareEntityKind::Component, "core");
    let package = entity(SoftwareEntityKind::PackageComponent, "serde");
    let mut candidate = statement(&component, &package);
    candidate.predicate = SoftwarePredicate::DependsOn;
    candidate.evidence_refs = vec![evidence("other-scope")];

    let (statements, report) =
        reconcile_software_statements(&[component, package], vec![candidate]);

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "cross_scope_evidence")
    );
    assert_eq!(statements[0].fact_state, SoftwareFactState::Rejected);
}

#[test]
fn owl_object_properties_reject_literal_objects() {
    let component = entity(SoftwareEntityKind::Component, "core");
    let package = entity(SoftwareEntityKind::PackageComponent, "serde");
    let mut candidate = statement(&component, &package);
    candidate.predicate = SoftwarePredicate::DependsOn;
    candidate.object_id = None;
    candidate.object_value = Some("serde".to_owned());

    let (statements, report) = reconcile_software_statements(&[component], vec![candidate]);

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "literal_object_for_object_property")
    );
    assert_eq!(statements[0].fact_state, SoftwareFactState::Rejected);
}
