use rusqlite::{Connection, params};

use super::super::schema::initialize_schema;
use super::test_support::*;
use super::*;

#[test]
fn projection_query_filters_kind_without_unrelated_graph_staleness() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");
    connection
        .execute("UPDATE graph_state SET graph_version = 2 WHERE id = 1", [])
        .expect("graph version should update");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Sdks,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");

    assert!(!projection.status.stale);
    assert!(projection.components.is_empty());
    assert_eq!(projection.sdk_usages.len(), 2);
}

#[test]
fn projection_all_kind_excludes_diagnostics_outside_the_requested_evidence_filter() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_symbols (
                repository_id, source_scope, symbol_snapshot_id, path, language_id,
                name, kind, line_start, line_end
            ) VALUES
                ('repo', 'scope-1', 'symbol-api', 'src/api.rs', 'rust',
                 'GraphApi', 'trait', 4, 20),
                ('repo', 'scope-1', 'symbol-test', 'tests/lifecycle.rs', 'rust',
                 'lifecycle_smoke_test', 'function', 8, 16)",
            [],
        )
        .expect("ontology symbols should insert");
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");
    let in_scope_entity = connection
        .query_row(
            "SELECT entity_key FROM software_entities WHERE primary_evidence_path = 'src/api.rs'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("source entity should materialize");
    let out_of_scope_entity = connection
        .query_row(
            "SELECT entity_key FROM software_entities WHERE primary_evidence_path = 'tests/lifecycle.rs'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("test entity should materialize");
    connection
        .execute(
            "INSERT INTO software_ontology_diagnostics (
                diagnostic_id, source_scope, shape_id, code, severity, statement_id,
                entity_key, field, message
            ) VALUES ('diagnostic-outside-src', 'scope-1', 'shape', 'a-outside', 'error',
                      NULL, ?1, 'entity_key', 'outside requested path')",
            params![out_of_scope_entity],
        )
        .expect("out-of-scope diagnostic should insert");
    connection
        .execute(
            "INSERT INTO software_ontology_diagnostics (
                diagnostic_id, source_scope, shape_id, code, severity, statement_id,
                entity_key, field, message
            ) VALUES ('diagnostic-inside-src', 'scope-1', 'shape', 'z-inside', 'warning',
                      NULL, ?1, 'entity_key', 'inside requested path')",
            params![in_scope_entity],
        )
        .expect("in-scope diagnostic should insert");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit-1",
            vec!["src".to_owned()],
            vec!["rust".to_owned()],
        )
        .expect("selector should validate"),
        SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::AllowStale,
        12,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");

    assert_eq!(
        projection
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.diagnostic_id.as_str())
            .collect::<Vec<_>>(),
        vec!["diagnostic-inside-src"]
    );
}

#[test]
fn all_slice_budget_uses_fixed_round_robin_priority_and_redistributes_capacity() {
    assert_eq!(
        fair_limit::round_robin_slice_budgets([8; 12], 4),
        [1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        fair_limit::round_robin_slice_budgets([0, 2, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0], 3),
        [0, 2, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        fair_limit::round_robin_slice_budgets([8; 12], 16),
        [2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1]
    );
}

#[test]
fn projection_all_kind_applies_small_limit_across_response_arrays() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::AllowStale,
        4,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");
    let slice_lengths = [
        projection.components.len(),
        projection.dependency_usages.len(),
        projection.sdk_usages.len(),
        projection.files.len(),
        projection.topics.len(),
        projection.relationships.len(),
        projection.build_targets.len(),
        projection.iac_resources.len(),
        projection.design_elements.len(),
        projection.entities.len(),
        projection.statements.len(),
        projection.diagnostics.len(),
    ];

    assert_eq!(slice_lengths.iter().sum::<usize>(), 4);
    assert_eq!(projection.components.len(), 1);
    assert_eq!(projection.sdk_usages.len(), 1);
    assert_eq!(projection.files.len(), 1);
    assert_eq!(projection.relationships.len(), 1);
}

#[test]
fn projection_all_kind_keeps_components_referenced_by_returned_dependency_usages() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES (
                'repo', 'scope-1', 'handwritten-rust', 'src/lib.rs', 'rust', 'parsed'
            )",
            [],
        )
        .expect("handwritten file should insert");
    connection
        .execute(
            "INSERT INTO code_repository_imports (
                repository_id, source_scope, file_id, path, module, target_hint,
                resolution_state, confidence_basis_points, line_start, line_end
            ) VALUES (
                'repo', 'scope-1', 'handwritten-rust', 'src/lib.rs',
                'use serde::Serialize;', 'use serde::Serialize;', 'external', 9000, 1, 1
            )",
            [],
        )
        .expect("handwritten import should insert");
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::AllowStale,
        2,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");

    assert_eq!(projection.components.len(), 1);
    assert_eq!(projection.dependency_usages.len(), 1);
    assert!(projection.components.iter().any(|component| {
        component.component_id == projection.dependency_usages[0].component_id
    }));
}

#[test]
fn projection_query_rejects_unindexed_refs() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let missing_ref = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "missing-commit",
            Vec::new(),
            Vec::new(),
        )
        .expect("selector"),
        SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");
    let missing_ref_error =
        projection(&mut connection, missing_ref).expect_err("missing ref should fail");
    assert!(
        missing_ref_error
            .to_string()
            .contains("does not have an indexed software projection scope")
    );
}

#[test]
fn refresh_projection_materializes_files_topics_and_config_relationships() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_documented_configuration(&connection);

    let refreshed =
        refresh_projection(&mut connection, "scope-1").expect("projection should refresh");
    assert_eq!(refreshed.status.file_count, 13);
    assert_eq!(refreshed.status.relationship_count, 6);

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::AllowStale,
        100,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");
    assert!(
        projection
            .files
            .iter()
            .any(|file| { file.path == "docs/runtime.md" && file.file_role == "documentation" })
    );
    assert!(
        projection
            .files
            .iter()
            .any(|file| { file.path == "config/flags.yaml" && file.file_role == "configuration" })
    );
    assert!(
        projection
            .files
            .iter()
            .any(|file| { file.path == "tests/smoke.rs" && file.file_role == "test" })
    );
    assert!(
        projection
            .files
            .iter()
            .any(|file| { file.path == "k8s/deployment.yaml" && file.file_role == "deployment" })
    );
    assert!(
        projection
            .files
            .iter()
            .any(|file| { file.path == "src/k8s/client.rs" && file.file_role == "source" })
    );
    assert!(
        projection
            .files
            .iter()
            .any(|file| { file.path == "src/kubernetes/api.go" && file.file_role == "source" })
    );
    assert!(
        projection
            .files
            .iter()
            .any(|file| { file.path == "uv.lock" && file.file_role == "dependency_manifest" })
    );
    assert!(projection.files.iter().any(|file| {
        file.path == "build.gradle.kts" && file.file_role == "dependency_manifest"
    }));
    assert!(
        projection.files.iter().any(|file| {
            file.path == "CMakeLists.txt" && file.file_role == "dependency_manifest"
        })
    );
    assert!(projection.files.iter().any(|file| {
        file.path == "templates/deployment.yaml.j2" && file.file_role == "template"
    }));
    assert!(projection.topics.iter().any(|topic| {
        topic.name == "Runtime Configuration" && topic.topic_kind == "document_heading"
    }));
    assert!(projection.relationships.iter().any(|relationship| {
        relationship.relationship_kind == "documents"
            && relationship.target_kind == "topic"
            && relationship.evidence_path == "docs/runtime.md"
    }));
    assert!(projection.relationships.iter().any(|relationship| {
        relationship.relationship_kind == "configures"
            && relationship.target_kind == "configuration"
            && relationship.target_hint.as_deref() == Some("payments.enabled")
    }));
}

#[test]
fn projection_orders_operational_files_and_relationships_first() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_documented_configuration(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES ('repo', 'scope-1', 'workflow-file', '.github/workflows/ci.yml', 'yaml', 'parsed')",
            [],
        )
        .expect("workflow file should insert");
    connection
        .execute(
            "INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end
            ) VALUES (
                'repo', 'scope-1', 'github-actions', 'actions/checkout', 'v4',
                NULL, 'normal', 'manifest', 0, 'yaml', '.github/workflows/ci.yml', 9, 9
            )",
            [],
        )
        .expect("workflow action dependency should insert");
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let files = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Files,
        crate::domain::FreshnessPolicy::AllowStale,
        4,
    )
    .expect("request should validate");
    let file_projection = projection(&mut connection, files).expect("projection should load");
    assert_eq!(file_projection.files[0].path, "Cargo.toml");
    assert_eq!(file_projection.files[0].file_role, "dependency_manifest");

    let relationships = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Relationships,
        crate::domain::FreshnessPolicy::AllowStale,
        4,
    )
    .expect("request should validate");
    let relationship_projection =
        projection(&mut connection, relationships).expect("projection should load");
    assert_eq!(
        relationship_projection.relationships[0].relationship_kind,
        "depends_on"
    );
    assert_eq!(
        relationship_projection.relationships[0].evidence_path,
        "Cargo.toml"
    );
    assert_eq!(
        relationship_projection.relationships[0]
            .target_hint
            .as_deref(),
        Some("serde")
    );
}

#[test]
fn projection_materializes_api_schema_provenance_before_code_contracts() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    connection
        .execute_batch(
            "INSERT INTO code_repository_files (
                 repository_id, source_scope, file_id, path, language_id, parse_status
             ) VALUES
                 ('repo', 'scope-1', 'schema-file', 'spec/catalog.openapi.yaml', 'yaml', 'parsed'),
                 ('repo', 'scope-1', 'api-code-file', 'src/api.rs', 'rust', 'parsed');
             INSERT INTO code_repository_symbols (
                 repository_id, source_scope, symbol_snapshot_id, path, language_id,
                 name, kind, line_start, line_end
             ) VALUES (
                 'repo', 'scope-1', 'api-code-symbol', 'src/api.rs', 'rust',
                 'GraphApi', 'trait', 1, 3
             );",
        )
        .expect("API schema and code contract should insert");
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Apis,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("APIs should load");

    assert_eq!(projection.entities.len(), 2);
    assert_eq!(projection.entities[0].name, "spec/catalog.openapi.yaml");
    assert_eq!(
        projection.entities[0].source_kind,
        crate::domain::SoftwareSourceKind::ApiSchema
    );
    assert_eq!(projection.entities[1].name, "GraphApi");
    assert_eq!(
        projection.entities[1].source_kind,
        crate::domain::SoftwareSourceKind::Code
    );
}

#[test]
fn projection_orders_build_manifests_before_source_files() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES
                ('repo', 'scope-1', 'build-make', 'Makefile', 'make', 'parsed'),
                ('repo', 'scope-1', 'source-lib', 'src/lib.rs', 'rust', 'parsed')",
            [],
        )
        .expect("build manifest and source files should insert");
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let files = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Files,
        crate::domain::FreshnessPolicy::AllowStale,
        3,
    )
    .expect("request should validate");
    let file_projection = projection(&mut connection, files).expect("projection should load");
    let paths = file_projection
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(paths, ["Cargo.toml", "Makefile", "src/lib.rs"]);
}

#[test]
fn projection_orders_lifecycle_deployable_surfaces_first() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_lifecycle_projection_rows(&connection);

    let build = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Build,
        crate::domain::FreshnessPolicy::AllowStale,
        4,
    )
    .expect("request should validate");
    let build_projection = projection(&mut connection, build).expect("projection should load");
    assert_eq!(build_projection.build_targets[0].ecosystem, "npm");
    assert_eq!(build_projection.build_targets[0].kind, "script");
    assert_eq!(build_projection.build_targets[0].name, "build");

    let iac = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Iac,
        crate::domain::FreshnessPolicy::AllowStale,
        4,
    )
    .expect("request should validate");
    let iac_projection = projection(&mut connection, iac).expect("projection should load");
    assert_eq!(iac_projection.iac_resources[0].provider, "kubernetes");
    assert_eq!(iac_projection.iac_resources[0].resource_kind, "Deployment");
    assert_eq!(iac_projection.iac_resources[0].name, "relay-api");
    assert_eq!(iac_projection.iac_resources[1].provider, "kubernetes");
    assert_eq!(iac_projection.iac_resources[1].resource_kind, "Service");
    assert_eq!(iac_projection.iac_resources[1].name, "relay-service");
}

#[test]
fn refresh_projection_skips_empty_package_script_commands() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES ('repo', 'scope-1', 'package-json', 'frontend/package.json', 'json', 'parsed')",
            [],
        )
        .expect("package file should insert");
    connection
        .execute(
            "INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, path, language_id, content,
                line_start, line_end
            ) VALUES ('repo', 'scope-1', 'package-json-chunk', 'frontend/package.json',
                'json', ?1, 1, 8)",
            [r#"{
  "name": "frontend",
  "scripts": {
    "empty": "",
    "build": "vite build"
  }
}"#],
        )
        .expect("package chunk should insert");

    let projection =
        refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    assert!(projection.build_targets.iter().any(|target| {
        target.name == "build" && target.command.as_deref() == Some("vite build")
    }));
    assert!(
        !projection
            .build_targets
            .iter()
            .any(|target| target.name == "empty")
    );
}

#[test]
fn projection_configuration_relationship_targets_preserve_source_identity() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_documented_configuration(&connection);
    seed_environment_configuration_source(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Relationships,
        crate::domain::FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");
    let matching_targets = projection
        .relationships
        .iter()
        .filter(|relationship| {
            relationship.relationship_kind == "configures"
                && relationship.target_hint.as_deref() == Some("payments.enabled")
        })
        .map(|relationship| relationship.target_id.as_str())
        .collect::<Vec<_>>();

    assert!(matching_targets.contains(&"flag-config-payments-enabled"));
    assert!(matching_targets.contains(&"flag-env-payments-enabled"));
}

#[test]
fn projection_relationships_apply_language_filters_to_evidence_files() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_documented_configuration(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit-1",
            Vec::new(),
            vec!["rust".to_owned()],
        )
        .expect("selector"),
        SoftwareGlobalKind::Relationships,
        crate::domain::FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");

    assert_eq!(projection.relationships.len(), 2);
    assert!(projection.relationships.iter().any(|relationship| {
        relationship.relationship_kind == "depends_on"
            && relationship.evidence_path == "Cargo.toml"
            && relationship.target_hint.as_deref() == Some("serde")
    }));
    assert!(projection.relationships.iter().any(|relationship| {
        relationship.relationship_kind == "configures"
            && relationship.evidence_path == "src/lib.rs"
            && relationship.target_hint.as_deref() == Some("payments.enabled")
    }));
}

#[test]
fn refresh_projection_reads_knowledge_map_topics_from_symbols() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_knowledge_map_symbol(&connection);

    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");
    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::All,
        crate::domain::FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");

    assert!(
        projection.topics.iter().any(|topic| {
            topic.topic_kind == "knowledge_map_topic" && topic.name == "late-topic"
        })
    );
    assert!(projection.relationships.iter().any(|relationship| {
        relationship.relationship_kind == "documents"
            && relationship.evidence_path == ".knowledge/knowledge-map.yaml"
            && relationship.target_hint.as_deref() == Some("late-topic")
    }));
}

#[test]
fn refresh_projection_pages_knowledge_map_topic_symbols() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_knowledge_map_symbols(&connection, 513);

    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");
    let topic_count: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM software_topics
             WHERE source_scope = 'scope-1'
               AND topic_kind = 'knowledge_map_topic'",
            [],
            |row| row.get(0),
        )
        .expect("topic count should load");
    let relationship_count: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM software_relationships
             WHERE source_scope = 'scope-1'
               AND relationship_kind = 'documents'
               AND evidence_path = '.knowledge/knowledge-map.yaml'",
            [],
            |row| row.get(0),
        )
        .expect("relationship count should load");

    assert_eq!(topic_count, 513);
    assert_eq!(relationship_count, 513);
}

#[test]
fn projection_topics_apply_language_filters_to_source_files() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_documented_configuration(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let rust_topics = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit-1",
            Vec::new(),
            vec!["rust".to_owned()],
        )
        .expect("selector"),
        SoftwareGlobalKind::Topics,
        crate::domain::FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request should validate");
    let rust_projection = projection(&mut connection, rust_topics).expect("projection should load");
    assert!(rust_projection.topics.is_empty());

    let markdown_topics = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit-1",
            Vec::new(),
            vec!["markdown".to_owned()],
        )
        .expect("selector"),
        SoftwareGlobalKind::Topics,
        crate::domain::FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request should validate");
    let markdown_projection =
        projection(&mut connection, markdown_topics).expect("projection should load");
    assert_eq!(markdown_projection.topics.len(), 1);
    assert_eq!(markdown_projection.topics[0].name, "Runtime Configuration");
}
