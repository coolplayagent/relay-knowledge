use std::collections::BTreeMap;

use crate::domain::{
    GraphVersion, SoftwareAssertionMode, SoftwareEntity, SoftwareEntityKind, SoftwareFactState,
    SoftwarePredicate, SoftwareSourceKind, SoftwareStatement, SoftwareStatementResolution,
};

use super::{ProjectionSlices, apply_fair_total_limit};

#[test]
fn statement_budget_replaces_unrelated_entities_with_its_resolved_endpoints() {
    let mut slices = ProjectionSlices {
        entities: vec![entity("unrelated"), entity("subject"), entity("object")],
        statements: vec![statement("subject", Some("object"))],
        ..ProjectionSlices::default()
    };

    apply_fair_total_limit(&mut slices, 3);

    assert_eq!(slices.entities.len(), 2);
    assert_eq!(slices.statements.len(), 1);
    assert!(
        slices
            .entities
            .iter()
            .any(|entity| entity.entity_key == slices.statements[0].subject_id)
    );
    assert!(
        slices.entities.iter().any(|entity| {
            entity.entity_key == slices.statements[0].object_id.as_deref().unwrap()
        })
    );
}

#[test]
fn statement_budget_keeps_the_first_representable_statement_and_recovers_entity_capacity() {
    let mut slices = ProjectionSlices {
        entities: vec![entity("previous"), entity("subject"), entity("object")],
        statements: vec![
            statement("previous", None),
            statement("subject", Some("object")),
        ],
        ..ProjectionSlices::default()
    };

    apply_fair_total_limit(&mut slices, 4);

    assert_eq!(slices.statements.len(), 1);
    assert_eq!(slices.statements[0].subject_id, "previous");
    assert_eq!(
        slices
            .entities
            .iter()
            .map(|entity| entity.entity_key.as_str())
            .collect::<Vec<_>>(),
        vec!["previous", "subject", "object"]
    );
}

#[test]
fn statement_budget_replaces_duplicate_endpoint_occurrences_when_one_remains() {
    let mut slices = ProjectionSlices {
        entities: vec![
            entity_with_occurrence("subject", "first"),
            entity_with_occurrence("subject", "second"),
            entity("object"),
        ],
        statements: vec![statement("subject", Some("object"))],
        ..ProjectionSlices::default()
    };

    apply_fair_total_limit(&mut slices, 3);

    assert_eq!(slices.statements.len(), 1);
    assert_eq!(
        slices
            .entities
            .iter()
            .map(|entity| entity.entity_key.as_str())
            .collect::<Vec<_>>(),
        vec!["subject", "object"]
    );
}

#[test]
fn rejected_statements_redistribute_capacity_to_remaining_entities() {
    let mut slices = ProjectionSlices {
        entities: vec![
            entity("unrelated"),
            entity("first-subject"),
            entity("first-object"),
            entity("second-subject"),
            entity("second-object"),
        ],
        statements: vec![
            statement("first-subject", Some("first-object")),
            statement("second-subject", Some("second-object")),
        ],
        ..ProjectionSlices::default()
    };

    apply_fair_total_limit(&mut slices, 4);

    assert_eq!(slices.statements.len(), 1);
    assert_eq!(slices.statements[0].subject_id, "first-subject");
    assert_eq!(
        slices
            .entities
            .iter()
            .map(|entity| entity.entity_key.as_str())
            .collect::<Vec<_>>(),
        vec!["unrelated", "first-subject", "first-object"]
    );
}

#[test]
fn statement_budget_skips_unrepresentable_candidates_to_fill_its_allocation() {
    let mut slices = ProjectionSlices {
        entities: vec![entity("available")],
        statements: vec![statement("unavailable", None), statement("available", None)],
        ..ProjectionSlices::default()
    };

    apply_fair_total_limit(&mut slices, 2);

    assert_eq!(slices.statements.len(), 1);
    assert_eq!(slices.statements[0].subject_id, "available");
}

fn entity(entity_key: &str) -> SoftwareEntity {
    entity_with_occurrence(entity_key, entity_key)
}

fn entity_with_occurrence(entity_key: &str, occurrence_id: &str) -> SoftwareEntity {
    SoftwareEntity {
        entity_key: entity_key.to_owned(),
        occurrence_id: format!("occurrence-{occurrence_id}"),
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        entity_kind: SoftwareEntityKind::Component,
        name: entity_key.to_owned(),
        namespace: None,
        source_kind: SoftwareSourceKind::Code,
        evidence_refs: Vec::new(),
        attributes: BTreeMap::new(),
        created_graph_version: GraphVersion::new(1),
    }
}

fn statement(subject_id: &str, object_id: Option<&str>) -> SoftwareStatement {
    SoftwareStatement {
        statement_id: "statement".to_owned(),
        subject_id: subject_id.to_owned(),
        predicate: SoftwarePredicate::Contains,
        object_id: object_id.map(str::to_owned),
        object_value: None,
        source_scope: "scope".to_owned(),
        source_kind: SoftwareSourceKind::Code,
        evidence_refs: Vec::new(),
        assertion_mode: SoftwareAssertionMode::Extracted,
        resolution_state: SoftwareStatementResolution::Resolved,
        valid_from: None,
        valid_to: None,
        observed_at: None,
        extractor_id: "test".to_owned(),
        extractor_version: "1".to_owned(),
        confidence_basis_points: 10_000,
        fact_state: SoftwareFactState::Active,
    }
}
