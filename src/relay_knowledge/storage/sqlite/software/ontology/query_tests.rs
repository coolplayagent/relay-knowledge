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

fn request(kind: SoftwareGlobalKind) -> SoftwareGlobalRequest {
    SoftwareGlobalRequest::new(
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
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
            ) VALUES (?1, ?2, 'repo', 'scope', ?3, ?4, ?5, ?6, ?7, 'rust', ?8, '{}', 1)
            ",
            params![
                id,
                format!("entity-{id}"),
                entity_kind,
                name,
                namespace,
                source_kind,
                evidence_path,
                evidence
            ],
        )
        .expect("entity should seed");
}
