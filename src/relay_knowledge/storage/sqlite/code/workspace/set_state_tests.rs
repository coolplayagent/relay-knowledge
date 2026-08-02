use super::super::{
    resolve_workspace_imports,
    test_support::{
        insert_scope, insert_source_file, insert_unresolved_import, workspace,
        workspace_schema_connection,
    },
};
use crate::domain::CodeMonorepoWorkspaceFormat;

#[test]
fn empty_workspaces_clear_previous_workspace_state() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    insert_unresolved_import(&transaction, "scope-main", "import-core", "@scope/core");
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
    resolve_workspace_imports(&transaction, &[], "repo", "scope-main")
        .expect("empty workspace result should clear state");

    for table in [
        "code_repository_sets",
        "code_repository_set_members",
        "code_workspace_package_mappings",
        "code_repository_cross_edges",
        "code_repository_set_overlay_status",
    ] {
        let count: u32 = transaction
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("table count");
        assert_eq!(count, 0, "{table} should be cleared");
    }
}

#[test]
fn marks_overlay_fresh_even_without_edges() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");

    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");

    let status = transaction
        .query_row(
            "SELECT state, edge_count, member_versions_json
             FROM code_repository_set_overlay_status",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .expect("overlay status should exist");

    assert_eq!(status.0, "fresh");
    assert_eq!(status.1, 0);
    assert!(status.2.contains("scope-main"));
    assert!(status.2.contains("tree"));
}

#[test]
fn auto_workspace_set_does_not_reuse_user_workspace_alias() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");
    transaction
        .execute(
            "INSERT INTO code_repository_sets
             (set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms)
             VALUES ('user-set', 'repo-workspace', 'user managed', '{}', 1, 1)",
            [],
        )
        .expect("user set should insert");

    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("auto workspace should not collide with user set alias");

    let aliases = {
        let mut statement = transaction
            .prepare("SELECT alias FROM code_repository_sets ORDER BY alias")
            .expect("prepare aliases");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query aliases")
            .collect::<Result<Vec<_>, _>>()
            .expect("aliases")
    };
    assert_eq!(
        aliases,
        vec![
            "repo-auto-workspace".to_owned(),
            "repo-workspace".to_owned()
        ]
    );
}

#[test]
fn auto_workspace_member_ref_selector_uses_indexed_commit() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-main", "commit-main");

    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("auto workspace should resolve");

    let member_ref = transaction
        .query_row(
            "SELECT ref_selector, resolved_commit_sha
             FROM code_repository_set_members",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("member ref should exist");

    assert_eq!(
        member_ref,
        ("commit-main".to_owned(), "commit-main".to_owned())
    );
}

#[test]
fn resolving_new_scope_preserves_retained_scope_edges() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-old", "commit-old");
    insert_scope(&transaction, "scope-new", "commit-new");
    insert_unresolved_import(&transaction, "scope-old", "import-old", "@scope/core/old");
    insert_unresolved_import(&transaction, "scope-new", "import-new", "@scope/core/new");

    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-old",
    )
    .expect("old workspace imports should resolve");
    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-new",
    )
    .expect("new workspace imports should resolve");

    let mut member_statement = transaction
        .prepare(
            "SELECT source_scope, resolved_commit_sha
             FROM code_repository_set_members
             ORDER BY source_scope",
        )
        .expect("member statement");
    let members = member_statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("members should load")
        .collect::<Result<Vec<_>, _>>()
        .expect("members should collect");
    let old_edge_count: u32 = transaction
        .query_row(
            "SELECT COUNT(*) FROM code_repository_cross_edges
             WHERE from_source_scope = 'scope-old'",
            [],
            |row| row.get(0),
        )
        .expect("old edge count");
    let overlay_edge_count: u32 = transaction
        .query_row(
            "SELECT edge_count FROM code_repository_set_overlay_status",
            [],
            |row| row.get(0),
        )
        .expect("overlay edge count");

    assert_eq!(
        members,
        vec![
            ("scope-new".to_owned(), "commit-new".to_owned()),
            ("scope-old".to_owned(), "commit-old".to_owned())
        ]
    );
    assert_eq!(old_edge_count, 1);
    assert_eq!(overlay_edge_count, 2);
}

#[test]
fn clearing_empty_workspace_removes_only_current_retained_scope() {
    let mut connection = workspace_schema_connection();
    let transaction = connection.transaction().expect("transaction");
    insert_scope(&transaction, "scope-old", "commit-old");
    insert_scope(&transaction, "scope-new", "commit-new");
    insert_unresolved_import(&transaction, "scope-old", "import-old", "@scope/core/old");
    insert_unresolved_import(&transaction, "scope-new", "import-new", "@scope/core/new");

    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-old",
    )
    .expect("old workspace imports should resolve");
    resolve_workspace_imports(
        &transaction,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-new",
    )
    .expect("new workspace imports should resolve");
    resolve_workspace_imports(&transaction, &[], "repo", "scope-new")
        .expect("empty workspace should clear only the current scope");

    let mut member_statement = transaction
        .prepare("SELECT source_scope FROM code_repository_set_members ORDER BY source_scope")
        .expect("member statement");
    let remaining_members: Vec<String> = member_statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("members should load")
        .collect::<Result<Vec<_>, _>>()
        .expect("members should collect");
    let mut edge_statement = transaction
        .prepare(
            "SELECT from_source_scope
             FROM code_repository_cross_edges
             ORDER BY from_source_scope",
        )
        .expect("edge statement");
    let edge_scopes: Vec<String> = edge_statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("edges should load")
        .collect::<Result<Vec<_>, _>>()
        .expect("edges should collect");
    let overlay_edge_count: u32 = transaction
        .query_row(
            "SELECT edge_count FROM code_repository_set_overlay_status",
            [],
            |row| row.get(0),
        )
        .expect("overlay edge count");

    assert_eq!(remaining_members, vec!["scope-old".to_owned()]);
    assert_eq!(edge_scopes, vec!["scope-old".to_owned()]);
    assert_eq!(overlay_edge_count, 1);
}
