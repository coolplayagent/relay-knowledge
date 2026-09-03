use rusqlite::{Connection, params};

use crate::domain::{
    SoftwareAssertionMode, SoftwareFactState, SoftwareGlobalKind, SoftwareGlobalRequest,
    SoftwarePredicate, SoftwareSourceKind, SoftwareStatement, SoftwareStatementResolution,
};

use super::super::super::schema::initialize_schema;
use super::append_statement_targets;

#[test]
fn statement_targets_scan_past_filtered_endpoints_before_filling_the_limit() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_schema(&connection).expect("software schema should initialize");
    insert_entity(&connection, "missing", "other/missing.rs");
    insert_entity(&connection, "available", "src/available.rs");
    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit",
            vec!["src".to_owned()],
            Vec::new(),
        )
        .expect("selector should validate"),
        SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::AllowStale,
        1,
    )
    .expect("request should validate");
    let mut entities = Vec::new();

    append_statement_targets(
        &connection,
        "scope",
        &request,
        &mut entities,
        &[statement("missing"), statement("available")],
    )
    .expect("filtered statement endpoint should load");

    assert_eq!(
        entities
            .iter()
            .map(|entity| entity.entity_key.as_str())
            .collect::<Vec<_>>(),
        vec!["entity-available"]
    );
}

#[test]
fn statement_targets_restore_canonical_order_after_multiple_query_batches() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_schema(&connection).expect("software schema should initialize");
    let mut statements = Vec::new();
    for index in 0..256 {
        let id = format!("z-target-{index:03}");
        insert_entity(&connection, &id, "src/targets.rs");
        statements.push(statement(&id));
    }
    insert_entity(&connection, "a-target", "src/targets.rs");
    statements.push(statement("a-target"));
    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::AllowStale,
        500,
    )
    .expect("request should validate");
    let mut entities = Vec::new();

    append_statement_targets(&connection, "scope", &request, &mut entities, &statements)
        .expect("statement targets should load across query batches");

    assert_eq!(entities.len(), 257);
    assert_eq!(entities[0].entity_key, "entity-a-target");
    assert!(
        entities.windows(2).all(|pair| pair[0].name <= pair[1].name),
        "the concatenated query batches must preserve global canonical entity order"
    );
}

fn insert_entity(connection: &Connection, id: &str, evidence_path: &str) {
    let evidence = format!(
        r#"[{{"evidence_id":"evidence-{id}","source_scope":"scope","path":"{evidence_path}","line_range":{{"start":1,"end":1}}}}]"#
    );
    connection
        .execute(
            "
            INSERT INTO software_entities (
                occurrence_id, entity_key, repository_id, source_scope, entity_kind,
                name, namespace, source_kind, primary_evidence_path, language_id,
                evidence_refs_json, attributes_json, created_graph_version
            ) VALUES (?1, ?2, 'repo', 'scope', 'api', ?1, NULL, 'code', ?3, 'rust', ?4, '{}', 1)
            ",
            params![id, format!("entity-{id}"), evidence_path, evidence],
        )
        .expect("entity should insert");
}

fn statement(subject_id: &str) -> SoftwareStatement {
    SoftwareStatement {
        statement_id: format!("statement-{subject_id}"),
        subject_id: format!("entity-{subject_id}"),
        predicate: SoftwarePredicate::Contains,
        object_id: None,
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
