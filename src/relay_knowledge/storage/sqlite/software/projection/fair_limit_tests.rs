use std::collections::BTreeMap;

use crate::domain::{
    GraphVersion, RepositoryCodeRange, SoftwareAssertionMode, SoftwareComponent,
    SoftwareDependencyUsage, SoftwareEntity, SoftwareEntityKind, SoftwareFactState,
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
fn statement_budget_reclaims_surplus_capacity_for_its_resolved_endpoints() {
    let mut slices = ProjectionSlices {
        components: vec![component("first"), component("second")],
        entities: vec![entity("subject"), entity("object")],
        statements: vec![statement("subject", Some("object"))],
        ..ProjectionSlices::default()
    };

    apply_fair_total_limit(&mut slices, 4);

    assert_eq!(slices.components.len(), 1);
    assert_eq!(slices.entities.len(), 2);
    assert_eq!(slices.statements.len(), 1);
    assert_eq!(
        slices.components.len() + slices.entities.len() + slices.statements.len(),
        4
    );
    assert!(
        slices
            .entities
            .iter()
            .any(|entity| { entity.entity_key == slices.statements[0].subject_id })
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

#[test]
fn dependency_usage_target_selection_preserves_component_evidence_order() {
    let mut slices = ProjectionSlices {
        components: vec![
            component("unused-a"),
            component("used-b"),
            component("used-c"),
        ],
        dependency_usages: vec![dependency_usage("used-b"), dependency_usage("used-c")],
        ..ProjectionSlices::default()
    };

    apply_fair_total_limit(&mut slices, 4);

    assert_eq!(
        slices
            .components
            .iter()
            .map(|component| component.component_id.as_str())
            .collect::<Vec<_>>(),
        vec!["used-b", "used-c"]
    );
    assert_eq!(
        slices
            .dependency_usages
            .iter()
            .map(|usage| usage.component_id.as_str())
            .collect::<Vec<_>>(),
        vec!["used-b", "used-c"]
    );
}

#[test]
fn rejected_statements_redistribute_capacity_to_dependency_usage_candidates() {
    let mut slices = ProjectionSlices {
        components: vec![component("available")],
        dependency_usages: (0..8)
            .map(|index| dependency_usage(&format!("available-{index}")))
            .map(|mut usage| {
                usage.component_id = "available".to_owned();
                usage
            })
            .collect(),
        entities: vec![entity("available")],
        statements: (0..8).map(|_| statement("unavailable", None)).collect(),
        ..ProjectionSlices::default()
    };

    apply_fair_total_limit(&mut slices, 10);

    assert_eq!(slices.components.len(), 1);
    assert_eq!(slices.entities.len(), 1);
    assert!(slices.statements.is_empty());
    assert_eq!(slices.dependency_usages.len(), 8);
}

fn entity(entity_key: &str) -> SoftwareEntity {
    entity_with_occurrence(entity_key, entity_key)
}

fn component(component_id: &str) -> SoftwareComponent {
    SoftwareComponent {
        component_id: component_id.to_owned(),
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        ecosystem: "cargo".to_owned(),
        language_id: "rust".to_owned(),
        name: component_id.to_owned(),
        requirement: None,
        resolved_version: None,
        dependency_group: "normal".to_owned(),
        source_kind: "manifest".to_owned(),
        relationship_state: "declared".to_owned(),
        evidence_path: format!("{component_id}.toml"),
        evidence_line_range: RepositoryCodeRange { start: 1, end: 1 },
        confidence_basis_points: 10_000,
        created_graph_version: GraphVersion::new(1),
    }
}

fn dependency_usage(component_id: &str) -> SoftwareDependencyUsage {
    SoftwareDependencyUsage {
        usage_id: format!("usage-{component_id}"),
        component_id: component_id.to_owned(),
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        ecosystem: "cargo".to_owned(),
        package_name: component_id.to_owned(),
        language_id: "rust".to_owned(),
        module: component_id.to_owned(),
        target_hint: None,
        resolution_state: "resolved".to_owned(),
        evidence_path: format!("src/{component_id}.rs"),
        evidence_line_range: RepositoryCodeRange { start: 1, end: 1 },
        confidence_basis_points: 10_000,
        created_graph_version: GraphVersion::new(1),
    }
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
