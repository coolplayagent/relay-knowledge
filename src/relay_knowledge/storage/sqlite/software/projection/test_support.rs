use rusqlite::{Connection, params};

pub(super) fn create_test_schema(connection: &Connection) {
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
                language_filters_json TEXT NOT NULL,
                stale INTEGER NOT NULL DEFAULT 0,
                retiring INTEGER NOT NULL DEFAULT 0
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
                parse_status TEXT NOT NULL,
                is_generated INTEGER NOT NULL DEFAULT 0
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

pub(super) fn seed_scope(connection: &Connection) {
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

pub(super) fn seed_knowledge_map_symbol(connection: &Connection) {
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

pub(super) fn seed_knowledge_map_symbols(connection: &Connection, count: usize) {
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

pub(super) fn seed_knowledge_map_file(connection: &Connection) {
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

pub(super) fn seed_lifecycle_projection_rows(connection: &Connection) {
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

pub(super) fn seed_documented_configuration(connection: &Connection) {
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

pub(super) fn seed_environment_configuration_source(connection: &Connection) {
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
