use super::*;
use crate::domain::{CodeMonorepoWorkspaceFormat, CodeWorkspaceMember};

use super::super::{
    resolve_workspace_imports,
    test_support::{
        insert_scope, insert_source_file, insert_unresolved_import,
        insert_unresolved_import_with_language, workspace, workspace_schema_connection,
    },
};

#[test]
fn normalizes_workspace_member_paths_without_parent_traversal() {
    assert_eq!(
        normalized_workspace_member_path("./api"),
        Some("api".to_owned())
    );
    assert_eq!(
        normalized_workspace_member_path(".\\api\\server"),
        Some("api/server".to_owned())
    );
    assert_eq!(normalized_workspace_member_path("."), Some(String::new()));
    assert_eq!(normalized_workspace_member_path("../api"), None);
}

#[test]
fn workspace_member_paths_are_keyed_by_package_and_ecosystem() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import(&transaction, "scope-main", "import-core", "core/utils");
    insert_source_file(
        &transaction,
        "scope-main",
        "file-npm-package",
        "packages/core/package.json",
        "json",
    );
    insert_source_file(
        &transaction,
        "scope-main",
        "file-rust-package",
        "crates/core/Cargo.toml",
        "rust",
    );
    let mut pnpm = workspace(CodeMonorepoWorkspaceFormat::Pnpm);
    pnpm.members = vec![CodeWorkspaceMember {
        package_name: "core".to_owned(),
        relative_path: "packages/core".to_owned(),
    }];
    let mut cargo = workspace(CodeMonorepoWorkspaceFormat::CargoWorkspace);
    cargo.members = vec![CodeWorkspaceMember {
        package_name: "core".to_owned(),
        relative_path: "crates/core".to_owned(),
    }];

    resolve_workspace_imports(&transaction, &[pnpm, cargo], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let target_file: String = transaction
        .query_row(
            "SELECT to_record_id FROM code_repository_cross_edges
             WHERE from_record_id = 'import-core'",
            [],
            |row| row.get(0),
        )
        .expect("edge should exist");
    assert_eq!(target_file, "file-npm-package");
}

#[test]
fn root_workspace_member_targets_ecosystem_manifest() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &transaction,
        "scope-main",
        "import-root",
        "@scope/root",
        "file-app",
        "packages/app/src/main.ts",
        "typescript",
    );
    insert_source_file(
        &transaction,
        "scope-main",
        "file-root-cargo",
        "Cargo.toml",
        "rust",
    );
    insert_source_file(
        &transaction,
        "scope-main",
        "file-root-package",
        "package.json",
        "json",
    );
    let mut pnpm = workspace(CodeMonorepoWorkspaceFormat::Pnpm);
    pnpm.members = vec![
        CodeWorkspaceMember {
            package_name: "@scope/root".to_owned(),
            relative_path: ".".to_owned(),
        },
        CodeWorkspaceMember {
            package_name: "@scope/app".to_owned(),
            relative_path: "packages/app".to_owned(),
        },
    ];

    resolve_workspace_imports(&transaction, &[pnpm], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let target_file: String = transaction
        .query_row(
            "SELECT to_record_id FROM code_repository_cross_edges
             WHERE from_record_id = 'import-root'",
            [],
            |row| row.get(0),
        )
        .expect("edge should exist");

    assert_eq!(target_file, "file-root-package");
}

#[test]
fn go_workspace_target_file_uses_normalized_member_path() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &transaction,
        "scope-main",
        "import-api",
        "example.com/svc/api/client",
        "file-main",
        "cmd/app/main.go",
        "go",
    );
    insert_source_file(&transaction, "scope-main", "file-root-mod", "go.mod", "go");
    insert_source_file(
        &transaction,
        "scope-main",
        "file-api-mod",
        "api/go.mod",
        "go",
    );
    let mut go = workspace(CodeMonorepoWorkspaceFormat::GoModules);
    go.members[0].relative_path = "./api".to_owned();

    resolve_workspace_imports(&transaction, &[go], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let target_file: String = transaction
        .query_row(
            "SELECT to_record_id FROM code_repository_cross_edges
             WHERE from_record_id = 'import-api'",
            [],
            |row| row.get(0),
        )
        .expect("edge should exist");
    assert_eq!(target_file, "file-api-mod");
}
