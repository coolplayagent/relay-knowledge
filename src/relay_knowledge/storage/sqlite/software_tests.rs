use rusqlite::{Connection, params};

use super::*;

#[test]
fn refresh_projection_materializes_dependencies_and_unresolved_imports() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);

    let projection =
        refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    assert_eq!(projection.status.component_count, 3);
    assert_eq!(projection.status.sdk_usage_count, 2);
    assert!(
        projection.components.iter().any(
            |component| component.name == "serde" && component.relationship_state == "declared"
        )
    );
    assert!(
        projection
            .components
            .iter()
            .any(|component| component.name == "serde" && component.relationship_state == "locked")
    );
    assert_eq!(
        projection
            .components
            .iter()
            .filter(
                |component| component.name == "serde" && component.relationship_state == "declared"
            )
            .count(),
        2
    );
    assert_eq!(
        projection.sdk_usages[0].target_hint.as_deref(),
        Some("securec.h")
    );
}

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
fn projection_all_kind_keeps_combined_results_within_limit() {
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

    assert_eq!(projection.components.len() + projection.sdk_usages.len(), 4);
    assert_eq!(projection.components.len(), 3);
    assert_eq!(projection.sdk_usages.len(), 1);
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

#[test]
fn initialize_schema_marks_legacy_software_projection_status_stale() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "
            CREATE TABLE software_global_status (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                projected_graph_version INTEGER NOT NULL,
                stale INTEGER NOT NULL,
                component_count INTEGER NOT NULL,
                sdk_usage_count INTEGER NOT NULL,
                last_error TEXT
            );
            INSERT INTO software_global_status (
                source_scope, repository_id, projected_graph_version, stale,
                component_count, sdk_usage_count, last_error
            ) VALUES ('scope-legacy', 'repo', 7, 0, 2, 1, NULL);
            ",
        )
        .expect("legacy status should insert");

    initialize_schema(&connection).expect("software schema should initialize");
    let status = status_for_scope(&connection, "scope-legacy")
        .expect("status should load")
        .expect("status should exist");

    assert!(status.stale);
    assert_eq!(
        status.last_error.as_deref(),
        Some("software global projection schema changed; refresh required")
    );
}

#[test]
fn initialize_schema_indexes_software_files_by_source_path() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    initialize_schema(&connection).expect("software schema should initialize");

    let index_sql = connection
        .query_row(
            "
            SELECT sql
            FROM sqlite_master
            WHERE type = 'index'
              AND name = 'software_files_scope_path'
            ",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("source path index should exist");

    assert!(index_sql.contains("software_files(source_scope, path)"));
}

fn create_test_schema(connection: &Connection) {
    connection
        .execute_batch(
            "
            CREATE TABLE graph_state (id INTEGER PRIMARY KEY CHECK (id = 1), graph_version INTEGER NOT NULL);
            INSERT INTO graph_state (id, graph_version) VALUES (1, 1);
            CREATE TABLE code_repository_scopes (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                path_filters_json TEXT NOT NULL,
                language_filters_json TEXT NOT NULL
            );
            CREATE TABLE code_repositories (
                repository_id TEXT PRIMARY KEY,
                alias TEXT NOT NULL,
                last_indexed_scope_id TEXT
            );
            CREATE TABLE code_repository_aliases (
                alias TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL
            );
            CREATE TABLE code_repository_dependencies (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                ecosystem TEXT NOT NULL,
                package_name TEXT NOT NULL,
                requirement TEXT,
                resolved_version TEXT,
                dependency_group TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                is_lockfile INTEGER NOT NULL,
                language_id TEXT NOT NULL,
                path TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            CREATE TABLE code_repository_files (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                parse_status TEXT NOT NULL
            );
            CREATE TABLE code_repository_imports (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                module TEXT NOT NULL,
                target_hint TEXT,
                resolution_state TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            CREATE TABLE code_repository_symbols (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                symbol_snapshot_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            CREATE TABLE code_repository_chunks (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            CREATE TABLE code_repository_feature_flags (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                feature_flag_id TEXT NOT NULL,
                usage_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                name TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                source_key TEXT NOT NULL,
                edge_kind TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                confidence_tier TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            ",
        )
        .expect("test schema should initialize");
}

fn seed_scope(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha,
                path_filters_json, language_filters_json
            ) VALUES ('scope-1', 'repo', 'commit-1', '[]', '[]')",
            [],
        )
        .expect("scope should insert");
    connection
        .execute(
            "INSERT INTO code_repositories (repository_id, alias, last_indexed_scope_id) VALUES ('repo', 'core', 'scope-1')",
            [],
        )
        .expect("repo should insert");
    connection
        .execute(
            "INSERT INTO code_repository_aliases (alias, repository_id) VALUES ('core', 'repo')",
            [],
        )
        .expect("alias should insert");
    connection
        .execute(
            "INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end
            ) VALUES ('repo', 'scope-1', 'cargo', 'serde', '1', NULL, 'normal', 'manifest', 0, 'rust', 'Cargo.toml', 7, 7)",
            [],
        )
        .expect("manifest dependency should insert");
    connection
        .execute(
            "INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end
            ) VALUES ('repo', 'scope-1', 'cargo', 'serde', '1', NULL, 'normal', 'manifest', 0, 'rust', 'crates/core/Cargo.toml', 9, 9)",
            [],
        )
        .expect("duplicate manifest dependency should insert");
    connection
        .execute(
            "INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end
            ) VALUES ('repo', 'scope-1', 'cargo', 'serde', NULL, '1.0.0', 'normal', 'lockfile', 1, 'rust', 'Cargo.lock', 33, 33)",
            [],
        )
        .expect("lock dependency should insert");
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES
                ('repo', 'scope-1', 'file-1', 'src/main.cc', 'cpp', 'parsed'),
                ('repo', 'scope-1', 'manifest-cargo', 'Cargo.toml', 'toml', 'parsed')",
            [],
        )
        .expect("file should insert");
    connection
        .execute(
            "INSERT INTO code_repository_imports (
                repository_id, source_scope, file_id, path, module, target_hint,
                resolution_state, confidence_basis_points, line_start, line_end
            ) VALUES ('repo', 'scope-1', 'file-1', 'src/main.cc', '#include <securec.h>', 'securec.h', 'unresolved', 2500, 3, 3)",
            [],
        )
        .expect("import should insert");
    connection
        .execute(
            "INSERT INTO code_repository_imports (
                repository_id, source_scope, file_id, path, module, target_hint,
                resolution_state, confidence_basis_points, line_start, line_end
            ) VALUES ('repo', 'scope-1', 'file-1', 'src/main.cc', '#include <securec.h>', 'securec.h', 'unresolved', 2500, 9, 9)",
            [],
        )
        .expect("repeated import should insert");
}

fn seed_knowledge_map_symbol(connection: &Connection) {
    seed_knowledge_map_file(connection);
    connection
        .execute(
            "INSERT INTO code_repository_symbols (
                repository_id, source_scope, symbol_snapshot_id, path, language_id,
                name, kind, line_start, line_end
            ) VALUES (
                'repo', 'scope-1', 'topic-late', '.knowledge/knowledge-map.yaml', 'yaml',
                'late-topic', 'knowledge_map_topic', 4200, 4200
            )",
            [],
        )
        .expect("knowledge map topic symbol should insert");
}

fn seed_knowledge_map_symbols(connection: &Connection, count: usize) {
    seed_knowledge_map_file(connection);
    for index in 0..count {
        let line = u32::try_from(index + 1).expect("test line should fit");
        connection
            .execute(
                "INSERT INTO code_repository_symbols (
                    repository_id, source_scope, symbol_snapshot_id, path, language_id,
                    name, kind, line_start, line_end
                ) VALUES (
                    'repo', 'scope-1', ?1, '.knowledge/knowledge-map.yaml', 'yaml',
                    ?2, 'knowledge_map_topic', ?3, ?3
                )",
                params![
                    format!("topic-page-{index}"),
                    format!("topic-{index:03}"),
                    line
                ],
            )
            .expect("knowledge map topic symbol should insert");
    }
}

fn seed_knowledge_map_file(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES (
                'repo', 'scope-1', 'knowledge-map', '.knowledge/knowledge-map.yaml',
                'yaml', 'parsed'
            )",
            [],
        )
        .expect("knowledge map file should insert");
}

fn seed_lifecycle_projection_rows(connection: &Connection) {
    connection
        .execute_batch(
            "
            INSERT INTO software_build_targets (
                target_id, repository_id, source_scope, ecosystem, language_id, name,
                kind, command, output_hint, source_kind, evidence_path,
                evidence_line_start, evidence_line_end, confidence_basis_points,
                created_graph_version
            ) VALUES
                ('build-rust-package', 'repo', 'scope-1', 'rust', 'rust',
                 'relay-core', 'package', NULL, NULL, 'Cargo.toml',
                 'Cargo.toml', 1, 1, 9000, 1),
                ('build-cmake-exe', 'repo', 'scope-1', 'cmake', 'cmake',
                 'relay_agent', 'executable', NULL, NULL, 'CMakeLists.txt',
                 'CMakeLists.txt', 4, 4, 9000, 1),
                ('build-npm-script', 'repo', 'scope-1', 'npm', 'json',
                 'build', 'script', 'vite build', NULL, 'package.json',
                 'package.json', 8, 8, 9000, 1);

            INSERT INTO software_iac_resources (
                resource_id, repository_id, source_scope, language_id, provider,
                resource_kind, name, scope_hint, target_hint, resolution_state,
                source_kind, evidence_path, evidence_line_start, evidence_line_end,
                confidence_basis_points, created_graph_version
            ) VALUES
                ('iac-container-base', 'repo', 'scope-1', 'dockerfile', 'container',
                 'base_image', 'rust:1.76', NULL, 'rust:1.76', 'extracted',
                 'Dockerfile', 'Dockerfile', 1, 1, 9000, 1),
                ('iac-compose-web', 'repo', 'scope-1', 'yaml', 'compose',
                 'service', 'web', NULL, NULL, 'extracted',
                 'compose', 'docker-compose.yml', 3, 3, 9000, 1),
	                ('iac-kubernetes-api', 'repo', 'scope-1', 'yaml', 'kubernetes',
	                 'Deployment', 'relay-api', 'Deployment', NULL, 'extracted',
	                 'kubernetes-yaml', 'deploy/app.yaml', 4, 4, 9000, 1),
	                ('iac-kubernetes-service', 'repo', 'scope-1', 'yaml', 'kubernetes',
	                 'Service', 'relay-service', 'Service', NULL, 'extracted',
	                 'kubernetes-yaml', 'deploy/service.yaml', 4, 4, 9000, 1),
	                ('iac-kubernetes-resource', 'repo', 'scope-1', 'yaml', 'kubernetes',
	                 'resource', 'relay-custom', 'CustomResourceDefinition', NULL, 'extracted',
	                 'kubernetes-yaml', 'deploy/custom.yaml', 4, 4, 9000, 1);
	            ",
        )
        .expect("lifecycle projection rows should insert");
}

fn seed_documented_configuration(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES
                ('repo', 'scope-1', 'doc-1', 'docs/runtime.md', 'markdown', 'parsed'),
                ('repo', 'scope-1', 'config-1', 'config/flags.yaml', 'yaml', 'parsed'),
                ('repo', 'scope-1', 'code-1', 'src/lib.rs', 'rust', 'parsed'),
                ('repo', 'scope-1', 'test-1', 'tests/smoke.rs', 'rust', 'parsed'),
                ('repo', 'scope-1', 'deploy-1', 'k8s/deployment.yaml', 'yaml', 'parsed'),
                ('repo', 'scope-1', 'k8s-client', 'src/k8s/client.rs', 'rust', 'parsed'),
                ('repo', 'scope-1', 'kubernetes-api', 'src/kubernetes/api.go', 'go', 'parsed'),
                ('repo', 'scope-1', 'template-1', 'templates/deployment.yaml.j2', 'jinja2', 'parsed'),
                ('repo', 'scope-1', 'uv-lock', 'uv.lock', 'toml', 'parsed'),
                ('repo', 'scope-1', 'gradle-kts', 'build.gradle.kts', 'kotlin', 'parsed'),
                ('repo', 'scope-1', 'cmake-1', 'CMakeLists.txt', 'cmake', 'parsed')",
            [],
        )
        .expect("document and config files should insert");
    connection
        .execute(
            "INSERT INTO code_repository_symbols (
                repository_id, source_scope, symbol_snapshot_id, path, language_id,
                name, kind, line_start, line_end
            ) VALUES (
                'repo', 'scope-1', 'heading-1', 'docs/runtime.md', 'markdown',
                'Runtime Configuration', 'heading', 1, 1
            )",
            [],
        )
        .expect("heading should insert");
    connection
        .execute(
            "INSERT INTO code_repository_feature_flags (
                repository_id, source_scope, feature_flag_id, usage_id, path, language_id,
                name, source_kind, source_key, edge_kind, confidence_basis_points,
                confidence_tier, line_start, line_end
            ) VALUES
                ('repo', 'scope-1', 'flag-config-payments-enabled', 'flag-define',
                 'config/flags.yaml', 'yaml', 'payments.enabled', 'config_key',
                 'payments.enabled', 'defines_config', 10000, 'extracted', 2, 2),
                ('repo', 'scope-1', 'flag-config-payments-enabled', 'flag-read',
                 'src/lib.rs', 'rust', 'payments.enabled', 'config_key',
                 'payments.enabled', 'reads_config', 8000, 'inferred', 8, 8)",
            [],
        )
        .expect("feature flag relationships should insert");
}

fn seed_environment_configuration_source(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO code_repository_feature_flags (
                repository_id, source_scope, feature_flag_id, usage_id, path, language_id,
                name, source_kind, source_key, edge_kind, confidence_basis_points,
                confidence_tier, line_start, line_end
            ) VALUES (
                'repo', 'scope-1', 'flag-env-payments-enabled', 'flag-env-read',
                'src/lib.rs', 'rust', 'payments.enabled', 'env_var',
                'payments.enabled', 'reads_config', 8000, 'inferred', 9, 9
            )",
            [],
        )
        .expect("environment-backed feature flag should insert");
}
