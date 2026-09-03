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
fn statement_budget_never_replaces_an_endpoint_of_the_statement_being_retained() {
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
    assert!(
        slices
            .entities
            .iter()
            .all(|entity| entity.entity_key != "object")
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
    SoftwareEntity {
        entity_key: entity_key.to_owned(),
        occurrence_id: format!("occurrence-{entity_key}"),
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
