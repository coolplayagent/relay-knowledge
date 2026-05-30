use rusqlite::{Connection, params};

use super::*;

#[test]
fn projection_filters_rows_when_serving_broader_scope() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let narrow_dependencies = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit-1",
            vec!["crates/core".to_owned()],
            Vec::new(),
        )
        .expect("selector"),
        SoftwareGlobalKind::Dependencies,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");
    let dependency_projection =
        projection(&mut connection, narrow_dependencies).expect("broader scope should serve");
    assert_eq!(dependency_projection.components.len(), 1);
    assert_eq!(
        dependency_projection.components[0].evidence_path,
        "crates/core/Cargo.toml"
    );
    assert!(dependency_projection.dependency_usages.is_empty());

    let rust_dependencies = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit-1",
            Vec::new(),
            vec!["rust".to_owned()],
        )
        .expect("selector"),
        SoftwareGlobalKind::Dependencies,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");
    let rust_projection =
        projection(&mut connection, rust_dependencies).expect("scope should load");
    assert!(
        rust_projection
            .components
            .iter()
            .all(|component| component.language_id == "rust")
    );
    assert!(
        !rust_projection
            .components
            .iter()
            .any(|component| component.name == "react")
    );
    assert!(rust_projection.dependency_usages.is_empty());

    let rust_sdk = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new(
            "repo",
            "commit-1",
            vec!["src".to_owned()],
            vec!["rust".to_owned()],
        )
        .expect("selector"),
        SoftwareGlobalKind::Sdks,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, rust_sdk).expect("scope should load");
    assert!(projection.sdk_usages.is_empty());
}

#[test]
fn projection_links_declared_dependencies_to_import_usage() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Dependencies,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");
    let full_projection = projection(&mut connection, request).expect("scope should load");

    assert!(
        full_projection
            .dependency_usages
            .iter()
            .any(|usage| usage.package_name == "react"
                && usage.module == "import React from \"react\";"
                && usage.evidence_path == "src/app.js")
    );
    assert!(
        full_projection
            .dependency_usages
            .iter()
            .all(|usage| usage.package_name != "securec")
    );

    let limited = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Dependencies,
        crate::domain::FreshnessPolicy::AllowStale,
        3,
    )
    .expect("request should validate");
    let limited_projection = projection(&mut connection, limited).expect("scope should load");
    assert_eq!(
        limited_projection.components.len() + limited_projection.dependency_usages.len(),
        3
    );
}

#[test]
fn projection_materializes_build_iac_and_design_slices_from_indexed_chunks() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_lifecycle_chunks(&connection);
    let refreshed =
        refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

    assert!(refreshed.status.build_target_count >= 3);
    assert!(refreshed.status.iac_resource_count >= 3);
    assert!(refreshed.status.design_element_count >= 2);

    let build_request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Build,
        crate::domain::FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request should validate");
    let build_projection = projection(&mut connection, build_request).expect("scope should load");
    assert!(build_projection.components.is_empty());
    assert!(
        build_projection
            .build_targets
            .iter()
            .any(|target| target.ecosystem == "npm"
                && target.kind == "script"
                && target.name == "build")
    );
    assert!(
        build_projection
            .build_targets
            .iter()
            .any(|target| target.ecosystem == "rust" && target.kind == "package")
    );

    let iac_request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Iac,
        crate::domain::FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request should validate");
    let iac_projection = projection(&mut connection, iac_request).expect("scope should load");
    assert!(
        iac_projection
            .iac_resources
            .iter()
            .any(|resource| resource.provider == "terraform"
                && resource.resource_kind == "resource"
                && resource.name == "app")
    );
    assert!(
        iac_projection
            .iac_resources
            .iter()
            .any(|resource| resource.provider == "compose" && resource.name == "web")
    );

    let design_request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Design,
        crate::domain::FreshnessPolicy::AllowStale,
        20,
    )
    .expect("request should validate");
    let design_projection = projection(&mut connection, design_request).expect("scope should load");
    assert!(
        design_projection
            .design_elements
            .iter()
            .any(|element| element.element_kind == "architecture"
                && element.name == "Architecture")
    );
    assert!(
        design_projection
            .design_elements
            .iter()
            .any(|element| element.element_kind == "module" && element.name == "relay-core")
    );
}

#[test]
fn projection_resolves_repository_id_before_alias() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    seed_alias_collision_scope(&connection);
    refresh_projection(&mut connection, "scope-1").expect("projection should refresh");
    refresh_projection(&mut connection, "scope-core").expect("projection should refresh");

    let request = SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("core", "commit-1", Vec::new(), Vec::new())
            .expect("selector"),
        SoftwareGlobalKind::Dependencies,
        crate::domain::FreshnessPolicy::AllowStale,
        10,
    )
    .expect("request should validate");
    let projection = projection(&mut connection, request).expect("projection should load");

    assert_eq!(projection.status.repository_id, "core");
    assert!(
        projection
            .components
            .iter()
            .any(|component| component.name == "core-package")
    );
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
                line_end INTEGER NOT NULL,
                excerpt TEXT NOT NULL
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
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                content TEXT NOT NULL,
                line_start INTEGER NOT NULL
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
                path, line_start, line_end, excerpt
            ) VALUES ('repo', 'scope-1', 'cargo', 'serde', '1', NULL, 'normal', 'manifest', 0, 'rust', 'Cargo.toml', 7, 7, 'serde = \"1\"')",
            [],
        )
        .expect("manifest dependency should insert");
    connection
        .execute(
            "INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end, excerpt
            ) VALUES ('repo', 'scope-1', 'cargo', 'serde', '1', NULL, 'normal', 'manifest', 0, 'rust', 'crates/core/Cargo.toml', 9, 9, 'serde = \"1\"')",
            [],
        )
        .expect("duplicate manifest dependency should insert");
    connection
        .execute(
            "INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end, excerpt
            ) VALUES ('repo', 'scope-1', 'npm', 'react', '18', NULL, 'dependencies', 'manifest', 0, 'javascript', 'package.json', 11, 11, '\"react\": \"18\"')",
            [],
        )
        .expect("javascript dependency should insert");
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES ('repo', 'scope-1', 'file-1', 'src/main.cc', 'cpp', 'parsed')",
            [],
        )
        .expect("file should insert");
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id, parse_status
            ) VALUES ('repo', 'scope-1', 'file-2', 'src/app.js', 'javascript', 'parsed')",
            [],
        )
        .expect("javascript file should insert");
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
            ) VALUES ('repo', 'scope-1', 'file-2', 'src/app.js', 'import React from \"react\";', 'react', 'unresolved', 9000, 1, 1)",
            [],
        )
            .expect("javascript import should insert");
}

fn seed_lifecycle_chunks(connection: &Connection) {
    insert_chunk(
        connection,
        "chunk-cargo",
        "Cargo.toml",
        "toml",
        1,
        "[package]\nname = \"relay-core\"\n\n[features]\nserver = []\n",
    );
    insert_chunk(
        connection,
        "chunk-package",
        "package.json",
        "json",
        1,
        "{\n  \"name\": \"relay-web\",\n  \"scripts\": {\n    \"build\": \"vite build\",\n    \"test\": \"vitest\"\n  }\n}\n",
    );
    insert_chunk(
        connection,
        "chunk-compose",
        "docker-compose.yml",
        "yaml",
        1,
        "services:\n  web:\n    image: relay/web:latest\n",
    );
    insert_chunk(
        connection,
        "chunk-tf",
        "infra/main.tf",
        "unknown",
        1,
        "provider \"aws\" {}\nresource \"aws_ecs_service\" \"app\" {}\nmodule \"network\" {}\n",
    );
    insert_chunk(
        connection,
        "chunk-k8s",
        "deploy/app.yaml",
        "yaml",
        1,
        "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: relay-api\n",
    );
    insert_chunk(
        connection,
        "chunk-doc",
        "docs/architecture.md",
        "unknown",
        1,
        "# Architecture\nRelay core separates indexing from query serving.\n\n## Module relay-core\nOwns software projection refresh.\n",
    );
}

fn insert_chunk(
    connection: &Connection,
    chunk_id: &str,
    path: &str,
    language_id: &str,
    line_start: u32,
    content: &str,
) {
    connection
        .execute(
            "INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, file_id, path, language_id,
                content, line_start
            ) VALUES ('repo', 'scope-1', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                chunk_id,
                format!("file-{chunk_id}"),
                path,
                language_id,
                content,
                line_start
            ],
        )
        .expect("chunk should insert");
}

fn seed_alias_collision_scope(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha,
                path_filters_json, language_filters_json
            ) VALUES ('scope-core', 'core', 'commit-1', '[]', '[]')",
            [],
        )
        .expect("colliding scope should insert");
    connection
        .execute(
            "INSERT INTO code_repositories (repository_id, alias, last_indexed_scope_id) VALUES ('core', 'other', 'scope-core')",
            [],
        )
        .expect("colliding repo should insert");
    connection
        .execute(
            "INSERT INTO code_repository_aliases (alias, repository_id) VALUES ('other', 'core')",
            [],
        )
        .expect("colliding alias should insert");
    connection
        .execute(
            "INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end, excerpt
            ) VALUES ('core', 'scope-core', 'cargo', 'core-package', '1', NULL, 'normal', 'manifest', 0, 'rust', 'Cargo.toml', 7, 7, 'core-package = \"1\"')",
            [],
        )
        .expect("colliding dependency should insert");
}
