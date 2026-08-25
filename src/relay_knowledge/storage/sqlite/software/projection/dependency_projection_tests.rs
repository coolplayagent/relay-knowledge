use rusqlite::{Connection, params};

use super::super::schema::initialize_schema;
use super::test_support::{create_test_schema, seed_scope};
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
fn refresh_projection_keeps_generated_oversized_import_without_dependency_usage() {
    let mut connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id,
                parse_status, is_generated
            ) VALUES (
                'repo', 'scope-1', 'generated-vendor', 'dist/vendor.min.js',
                'javascript', 'parsed', 1
            )",
            [],
        )
        .expect("generated file should insert");
    let oversized = "x".repeat(32 * 1_024 + 1);
    connection
        .execute(
            "INSERT INTO code_repository_imports (
                repository_id, source_scope, file_id, path, module, target_hint,
                resolution_state, confidence_basis_points, line_start, line_end
            ) VALUES (
                'repo', 'scope-1', 'generated-vendor', 'dist/vendor.min.js',
                ?1, ?1, 'external', 9000, 1, 1
            )",
            params![&oversized],
        )
        .expect("generated import should insert");

    let projection = refresh_projection(&mut connection, "scope-1")
        .expect("generated import should not enter bounded dependency matching");
    let stored_module_bytes = connection
        .query_row(
            "SELECT length(module)
             FROM code_repository_imports
             WHERE source_scope = 'scope-1'
               AND path = 'dist/vendor.min.js'",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("stored generated import should load");

    assert_eq!(stored_module_bytes, oversized.len());
    assert!(
        projection.sdk_usages.iter().any(|usage| {
            usage.evidence_path == "dist/vendor.min.js" && usage.module == oversized
        })
    );
    assert!(
        projection
            .dependency_usages
            .iter()
            .all(|usage| usage.evidence_path != "dist/vendor.min.js")
    );
}

#[test]
fn refresh_projection_rolls_back_when_handwritten_import_exceeds_match_limit() {
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
    refresh_projection(&mut connection, "scope-1")
        .expect("initial dependency usage should project");
    let initial_usage_count = connection
        .query_row(
            "SELECT COUNT(*)
             FROM software_dependency_usages
             WHERE source_scope = 'scope-1'
               AND evidence_path = 'src/lib.rs'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("initial dependency usage count should load");
    let oversized = "x".repeat(32 * 1_024 + 1);
    connection
        .execute(
            "UPDATE code_repository_imports
             SET module = ?1, target_hint = ?1
             WHERE source_scope = 'scope-1'
               AND path = 'src/lib.rs'",
            params![&oversized],
        )
        .expect("handwritten import should become oversized");

    let error = refresh_projection(&mut connection, "scope-1")
        .expect_err("oversized handwritten import should fail dependency matching");
    let retained_usage_module = connection
        .query_row(
            "SELECT module
             FROM software_dependency_usages
             WHERE source_scope = 'scope-1'
               AND evidence_path = 'src/lib.rs'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("failed refresh should retain the prior dependency usage");
    let stored_import_bytes = connection
        .query_row(
            "SELECT length(module)
             FROM code_repository_imports
             WHERE source_scope = 'scope-1'
               AND path = 'src/lib.rs'",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("oversized code import should remain stored");

    assert_eq!(initial_usage_count, 1);
    assert!(matches!(error, StorageError::CapacityExceeded(message)
        if message.contains("import match text bytes")));
    assert_eq!(retained_usage_module, "use serde::Serialize;");
    assert_eq!(stored_import_bytes, oversized.len());
}

#[test]
fn locked_components_coalesce_across_paths_with_stable_evidence() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);
    connection
        .execute(
            "INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end
            ) VALUES (
                'repo', 'scope-1', 'cargo', 'serde', NULL, '1.0.0', 'normal',
                'lockfile', 1, 'rust', 'crates/core/Cargo.lock', 17, 17
            )",
            [],
        )
        .expect("duplicate locked evidence should insert");

    let components =
        dependency_components_with_limit(&connection, "scope-1", GraphVersion::new(1), 3)
            .expect("duplicate locked evidence should not consume component capacity");
    let raw_dependency_count = connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_dependencies WHERE source_scope = 'scope-1'",
            [],
            |row| row.get::<_, usize>(0),
        )
        .expect("raw dependency evidence count should load");
    let locked = components
        .iter()
        .filter(|component| component.relationship_state == "locked")
        .collect::<Vec<_>>();

    assert_eq!(raw_dependency_count, 4);
    assert_eq!(components.len(), 3);
    assert_eq!(locked.len(), 1);
    assert_eq!(locked[0].evidence_path, "Cargo.lock");
    assert_eq!(locked[0].evidence_line_range.start, 33);
    assert_eq!(
        components
            .iter()
            .filter(|component| component.relationship_state == "declared")
            .count(),
        2
    );
}

#[test]
fn distinct_locked_component_coordinates_reject_cap_plus_one() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    connection
        .execute_batch(
            "
            INSERT INTO code_repository_dependencies (
                repository_id, source_scope, ecosystem, package_name, requirement,
                resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                path, line_start, line_end
            ) VALUES
                ('repo', 'scope-cap', 'go', 'example.test/module', NULL, 'v1.0.0',
                 'locked', 'go.sum', 1, 'go', 'a/go.sum', 1, 1),
                ('repo', 'scope-cap', 'go', 'example.test/module', NULL, 'v1.1.0',
                 'locked', 'go.sum', 1, 'go', 'b/go.sum', 1, 1),
                ('repo', 'scope-cap', 'go', 'example.test/module', NULL, 'v2.0.0',
                 'locked', 'go.sum', 1, 'go', 'c/go.sum', 1, 1);
            ",
        )
        .expect("distinct locked coordinates should insert");

    let error = dependency_components_with_limit(&connection, "scope-cap", GraphVersion::new(1), 2)
        .expect_err("three distinct locked coordinates should exceed a two-component cap");
    let components =
        dependency_components_with_limit(&connection, "scope-cap", GraphVersion::new(1), 3)
            .expect("three distinct locked coordinates should fit their exact cap");

    assert!(matches!(error, StorageError::CapacityExceeded(message)
        if message.contains("dependency components")));
    assert_eq!(components.len(), 3);
}

#[test]
fn component_and_sdk_projection_inputs_reject_cap_plus_one() {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    create_test_schema(&connection);
    initialize_schema(&connection).expect("software schema should initialize");
    seed_scope(&connection);

    let component_error =
        dependency_components_with_limit(&connection, "scope-1", GraphVersion::new(1), 2)
            .expect_err("three dependency rows should exceed a two-row cap");
    assert!(
        matches!(component_error, StorageError::CapacityExceeded(message)
        if message.contains("dependency components"))
    );

    let sdk_error =
        unresolved_sdk_usages_with_limit(&connection, "scope-1", GraphVersion::new(1), 1)
            .expect_err("two unresolved imports should exceed a one-row cap");
    assert!(matches!(sdk_error, StorageError::CapacityExceeded(message)
        if message.contains("SDK usages")));
}
