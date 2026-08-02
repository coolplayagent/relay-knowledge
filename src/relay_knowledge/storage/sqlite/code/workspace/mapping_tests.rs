use super::*;
use crate::domain::{CodeMonorepoWorkspaceFormat, CodeWorkspaceMember};

use super::super::{
    resolve_workspace_imports,
    test_support::{insert_scope, workspace, workspace_schema_connection},
};

#[test]
fn exact_package_lookup_binds_package_after_set_id() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    transaction
        .execute(
            "INSERT INTO code_workspace_package_mappings
             (set_id, package_name, ecosystem, repository_id, source_scope,
              workspace_format, created_at_ms)
             VALUES ('set-1', '@scope/core', 'npm', 'repo', 'scope-core', 'pnpm', 1)",
            [],
        )
        .expect("insert mapping");

    let target = find_workspace_mapping_target(&transaction, "set-1", "@scope/core", "npm")
        .expect("lookup should not fail")
        .expect("exact package should resolve");

    assert_eq!(target.package_name, "@scope/core");
    assert_eq!(target.source_scope, "scope-core");
    assert!(
        find_workspace_mapping_target(&transaction, "set-1", "@scope/core", "rust")
            .expect("lookup should not fail")
            .is_none()
    );
}

#[test]
fn subpath_lookup_matches_longest_workspace_package_prefix() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    transaction
        .execute(
            "INSERT INTO code_workspace_package_mappings
             (set_id, package_name, ecosystem, repository_id, source_scope,
              workspace_format, created_at_ms)
             VALUES
                ('set-1', 'example.com/svc', 'go', 'repo', 'scope-svc', 'go_modules', 1),
                ('set-1', 'example.com/svc/api', 'go', 'repo', 'scope-api', 'go_modules', 1)",
            [],
        )
        .expect("insert mappings");

    let target =
        find_workspace_mapping_target(&transaction, "set-1", "example.com/svc/api/client", "go")
            .expect("lookup should not fail")
            .expect("subpath package should resolve");

    assert_eq!(target.package_name, "example.com/svc/api");
    assert_eq!(target.source_scope, "scope-api");
    assert_eq!(
        matches_workspace_package(&transaction, "set-1", "example.com/svc/api/client", "go",)
            .expect("package match should not fail"),
        Some("example.com/svc/api".to_owned())
    );
}

#[test]
fn workspace_mappings_allow_same_package_name_across_ecosystems() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
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

    let ecosystems: Vec<String> = {
        let mut statement = transaction
            .prepare(
                "SELECT ecosystem FROM code_workspace_package_mappings
                 WHERE package_name = 'core'
                 ORDER BY ecosystem",
            )
            .expect("prepare ecosystems");
        statement
            .query_map([], |row| row.get(0))
            .expect("query ecosystems")
            .collect::<Result<Vec<_>, _>>()
            .expect("ecosystems")
    };
    assert_eq!(ecosystems, vec!["npm".to_owned(), "rust".to_owned()]);
}

#[test]
fn replacing_workspace_package_mappings_prunes_removed_members() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    let full_workspace = workspace(CodeMonorepoWorkspaceFormat::Pnpm);
    let mut reduced_workspace = workspace(CodeMonorepoWorkspaceFormat::Pnpm);
    reduced_workspace
        .members
        .retain(|member| member.package_name != "@scope/core");

    resolve_workspace_imports(&transaction, &[full_workspace], "repo", "scope-main")
        .expect("full workspace imports should resolve");
    resolve_workspace_imports(&transaction, &[reduced_workspace], "repo", "scope-main")
        .expect("reduced workspace imports should resolve");

    let removed_count: u32 = transaction
        .query_row(
            "SELECT COUNT(*) FROM code_workspace_package_mappings
             WHERE package_name = '@scope/core'",
            [],
            |row| row.get(0),
        )
        .expect("removed mapping count");
    let remaining_count: u32 = transaction
        .query_row(
            "SELECT COUNT(*) FROM code_workspace_package_mappings
             WHERE package_name = '@scope/app'",
            [],
            |row| row.get(0),
        )
        .expect("remaining mapping count");

    assert_eq!(removed_count, 0);
    assert_eq!(remaining_count, 1);
}
