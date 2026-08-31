use rusqlite::{Connection, params};

use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn typed_entity_queries_prioritize_actionable_evidence() {
    let connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_schema(&connection)
        .expect("ontology schema should initialize");

    insert_entity(
        &connection,
        "api-doc",
        "api",
        "Catalog API",
        "markdown-metadata",
        "documentation",
        "docs/catalog.md",
    );
    insert_entity(
        &connection,
        "api-code",
        "api",
        "GraphApi",
        "src/lib.rs",
        "code",
        "src/lib.rs",
    );
    insert_entity(
        &connection,
        "resource-module",
        "resource",
        "network",
        "terraform",
        "iac",
        "infra/main.tf",
    );
    insert_entity(
        &connection,
        "resource-deployment",
        "resource",
        "relay-api",
        "kubernetes",
        "iac",
        "deploy/app.yaml",
    );
    insert_entity(
        &connection,
        "deployment-iac",
        "deployment_unit",
        "deploy/app.yaml",
        "kubernetes",
        "iac",
        "deploy/app.yaml",
    );
    insert_entity(
        &connection,
        "deployment-service",
        "deployment_unit",
        "service/relay.service",
        "systemd",
        "service_definition",
        "service/relay.service",
    );
    insert_entity(
        &connection,
        "runtime-service",
        "runtime_service",
        "relay",
        "systemd",
        "service_definition",
        "service/relay.service",
    );

    let apis = entities_for_scope(&connection, "scope", &request(SoftwareGlobalKind::Apis), 10)
        .expect("APIs should load");
    assert_eq!(
        apis.iter()
            .map(|entity| entity.name.as_str())
            .collect::<Vec<_>>(),
        ["GraphApi", "Catalog API"]
    );

    let resources = entities_for_scope(
        &connection,
        "scope",
        &request(SoftwareGlobalKind::Resources),
        10,
    )
    .expect("resources should load");
    assert_eq!(
        resources
            .iter()
            .map(|entity| entity.name.as_str())
            .collect::<Vec<_>>(),
        ["relay-api", "network"]
    );

    let deployments = entities_for_scope(
        &connection,
        "scope",
        &request(SoftwareGlobalKind::Deployments),
        10,
    )
    .expect("deployments should load");
    assert_eq!(
        deployments
            .iter()
            .map(|entity| entity.name.as_str())
            .collect::<Vec<_>>(),
        ["service/relay.service", "relay", "deploy/app.yaml"]
    );
}

#[test]
fn conflict_statement_query_applies_state_and_language_before_limit() {
    let connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_schema(&connection)
        .expect("ontology schema should initialize");
    insert_entity_with_language(
        &connection,
        "rust-subject",
        "software_system",
        "Rust subject",
        "code",
        "code",
        ("src/lib.rs", "rust"),
    );
    insert_entity_with_language(
        &connection,
        "yaml-subject",
        "software_system",
        "YAML subject",
        "kubernetes",
        "iac",
        ("deploy/app.yaml", "yaml"),
    );
    insert_statement(
        &connection,
        "active-resolved-rust",
        "entity-rust-subject",
        "resolved",
        "active",
        "src/lib.rs",
    );
    insert_statement(
        &connection,
        "active-unresolved-rust",
        "entity-rust-subject",
        "unresolved",
        "active",
        "src/lib.rs",
    );
    insert_statement(
        &connection,
        "conflicting-rust",
        "entity-rust-subject",
        "resolved",
        "conflicting",
        "src/lib.rs",
    );
    insert_statement(
        &connection,
        "conflicting-yaml",
        "entity-yaml-subject",
        "resolved",
        "conflicting",
        "deploy/app.yaml",
    );

    let conflicts = statements_for_scope(
        &connection,
        "scope",
        &request_with_languages(SoftwareGlobalKind::Conflicts, &["rust"]),
        10,
    )
    .expect("conflicting Rust statements should load");

    assert_eq!(
        conflicts
            .iter()
            .map(|statement| statement.statement_id.as_str())
            .collect::<Vec<_>>(),
        ["active-unresolved-rust", "conflicting-rust"]
    );
}

#[test]
fn diagnostic_query_decodes_rows_and_rejects_unknown_severity() {
    let connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_schema(&connection)
        .expect("ontology schema should initialize");
    connection
        .execute(
            "INSERT INTO software_ontology_diagnostics (
                 diagnostic_id, source_scope, shape_id, code, severity,
                 statement_id, entity_key, field, message
             ) VALUES ('diag', 'scope', 'shape', 'missing_field', 'warning',
                       'statement', 'entity', 'field', 'message')",
            [],
        )
        .expect("diagnostic should seed");

    let diagnostics =
        diagnostics_for_scope(&connection, "scope", 1).expect("diagnostic should decode");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].diagnostic_id, "diag");
    assert_eq!(diagnostics[0].severity, SoftwareShapeSeverity::Warning);
    assert_eq!(diagnostics[0].statement_id.as_deref(), Some("statement"));
    assert_eq!(diagnostics[0].entity_key.as_deref(), Some("entity"));

    connection
        .execute(
            "UPDATE software_ontology_diagnostics SET severity = 'unknown'",
            [],
        )
        .expect("diagnostic severity should update");
    let error = diagnostics_for_scope(&connection, "scope", 1)
        .expect_err("unknown persisted severity must fail closed");
    assert!(
        error
            .to_string()
            .contains("unknown software ontology value")
    );
}

#[test]
fn entity_query_rejects_invalid_persisted_evidence_json() {
    let connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_schema(&connection)
        .expect("ontology schema should initialize");
    insert_entity(
        &connection,
        "invalid-json",
        "api",
        "Invalid API",
        "code",
        "code",
        "src/lib.rs",
    );
    connection
        .execute(
            "UPDATE software_entities SET evidence_refs_json = '{' WHERE occurrence_id = 'invalid-json'",
            [],
        )
        .expect("evidence JSON should update");

    let error = entities_for_scope(&connection, "scope", &request(SoftwareGlobalKind::Apis), 1)
        .expect_err("invalid persisted evidence must fail closed");
    assert!(error.to_string().contains("invalid software ontology JSON"));
}

fn request(kind: SoftwareGlobalKind) -> SoftwareGlobalRequest {
    request_with_languages(kind, &[])
}

fn request_with_languages(kind: SoftwareGlobalKind, languages: &[&str]) -> SoftwareGlobalRequest {
    SoftwareGlobalRequest::new(
        CodeRepositorySelector::new(
            "repo",
            "commit",
            Vec::new(),
            languages
                .iter()
                .map(|language| (*language).to_owned())
                .collect(),
        )
        .expect("selector should validate"),
        kind,
        FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate")
}

fn insert_entity(
    connection: &Connection,
    id: &str,
    entity_kind: &str,
    name: &str,
    namespace: &str,
    source_kind: &str,
    evidence_path: &str,
) {
    insert_entity_with_language(
        connection,
        id,
        entity_kind,
        name,
        namespace,
        source_kind,
        (evidence_path, "rust"),
    );
}

fn insert_entity_with_language(
    connection: &Connection,
    id: &str,
    entity_kind: &str,
    name: &str,
    namespace: &str,
    source_kind: &str,
    evidence: (&str, &str),
) {
    let (evidence_path, language_id) = evidence;
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
            ) VALUES (?1, ?2, 'repo', 'scope', ?3, ?4, ?5, ?6, ?7, ?8, ?9, '{}', 1)
            ",
            params![
                id,
                format!("entity-{id}"),
                entity_kind,
                name,
                namespace,
                source_kind,
                evidence_path,
                language_id,
                evidence,
            ],
        )
        .expect("entity should seed");
}

fn insert_statement(
    connection: &Connection,
    statement_id: &str,
    subject_id: &str,
    resolution_state: &str,
    fact_state: &str,
    evidence_path: &str,
) {
    connection
        .execute(
            "INSERT INTO software_statements (
                 statement_id, source_scope, subject_id, predicate, object_id,
                 object_value, source_kind, evidence_refs_json, primary_evidence_path,
                 assertion_mode, resolution_state, valid_from, valid_to, observed_at,
                 extractor_id, extractor_version, confidence_basis_points, fact_state
             ) VALUES (?1, 'scope', ?2, 'depends_on', NULL, 'target', 'code', '[]', ?3,
                       'extracted', ?4, NULL, NULL, NULL, 'fixture', '1', 9000, ?5)",
            params![
                statement_id,
                subject_id,
                evidence_path,
                resolution_state,
                fact_state,
            ],
        )
        .expect("statement should seed");
}
