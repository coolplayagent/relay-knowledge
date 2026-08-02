use super::super::{
    resolve_workspace_imports,
    test_support::{
        insert_scope, insert_source_file, insert_unresolved_import,
        insert_unresolved_import_with_language, workspace, workspace_schema_connection,
    },
};
use crate::domain::{CodeMonorepoWorkspaceFormat, CodeWorkspaceMember};

#[test]
fn resolves_package_subpath_to_cross_workspace_edge() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import(
        &transaction,
        "scope-main",
        "import-core-utils",
        "@scope/core/utils",
    );
    insert_source_file(
        &transaction,
        "scope-main",
        "file-core-package",
        "packages/core/package.json",
        "json",
    );

    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");

    let edge = transaction
        .query_row(
            "SELECT to_source_scope, to_repository_id, resolution_state,
                    confidence_basis_points, confidence_tier, evidence_json,
                    to_record_kind, to_record_id
             FROM code_repository_cross_edges
             WHERE from_record_id = 'import-core-utils'",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u16>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .expect("edge should exist");

    assert_eq!(edge.0, Some("scope-main".to_owned()));
    assert_eq!(edge.1, Some("repo".to_owned()));
    assert_eq!(edge.2, "resolved");
    assert_eq!(edge.3, 10_000);
    assert_eq!(edge.4, "explicit");
    assert!(edge.5.contains("@scope/core"));
    assert!(edge.5.contains("from_line_start"));
    assert_eq!(edge.6, "code_file");
    assert_eq!(edge.7, Some("file-core-package".to_owned()));
}

#[test]
fn skips_self_imports_inside_target_member_path() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &transaction,
        "scope-main",
        "import-self-core",
        "core/utils",
        "file-core",
        "packages/core/src/index.ts",
        "typescript",
    );
    let mut pnpm = workspace(CodeMonorepoWorkspaceFormat::Pnpm);
    pnpm.members = vec![CodeWorkspaceMember {
        package_name: "core".to_owned(),
        relative_path: "packages/core".to_owned(),
    }];

    resolve_workspace_imports(&transaction, &[pnpm], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let edge_count: u32 = transaction
        .query_row(
            "SELECT COUNT(*) FROM code_repository_cross_edges",
            [],
            |row| row.get(0),
        )
        .expect("edge count");
    assert_eq!(edge_count, 0);
}

#[test]
fn go_lookup_strips_import_alias_tokens() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &transaction,
        "scope-main",
        "import-api",
        "api example.com/svc/api/client",
        "file-main",
        "cmd/app/main.go",
        "go",
    );
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

#[test]
fn skips_local_modules() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import(&transaction, "scope-main", "import-local", "./core");

    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");

    let edge_count: u32 = transaction
        .query_row(
            "SELECT COUNT(*) FROM code_repository_cross_edges",
            [],
            |row| row.get(0),
        )
        .expect("edge count");
    assert_eq!(edge_count, 0);
}

#[test]
fn skips_package_name_in_wrong_ecosystem() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &transaction,
        "scope-main",
        "import-rust-core",
        "@scope/core",
        "file-lib",
        "src/lib.rs",
        "rust",
    );
    insert_source_file(
        &transaction,
        "scope-main",
        "file-core-package",
        "packages/core/package.json",
        "json",
    );

    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");

    let edge_count: u32 = transaction
        .query_row(
            "SELECT COUNT(*) FROM code_repository_cross_edges",
            [],
            |row| row.get(0),
        )
        .expect("edge count");
    assert_eq!(edge_count, 0);
}
