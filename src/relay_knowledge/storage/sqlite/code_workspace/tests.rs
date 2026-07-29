use super::*;
use crate::domain::CodeWorkspaceMember;
use rusqlite::{Connection, Transaction, params};

fn workspace_schema_connection() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory connection");
    conn.execute_batch(
        "CREATE TABLE code_repository_sets (
            set_id TEXT PRIMARY KEY, alias TEXT NOT NULL UNIQUE,
            description TEXT, default_ref_policy_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL
        );
        CREATE TABLE code_repository_set_members (
            set_id TEXT NOT NULL, repository_id TEXT NOT NULL,
            repository_alias TEXT NOT NULL, ref_selector TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL, source_scope TEXT NOT NULL,
            path_filters_json TEXT NOT NULL, language_filters_json TEXT NOT NULL,
            priority INTEGER NOT NULL,
            PRIMARY KEY (set_id, repository_id, source_scope)
        );
        CREATE TABLE code_repository_scopes (
            source_scope TEXT PRIMARY KEY, repository_id TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL, tree_hash TEXT NOT NULL,
            path_filters_json TEXT NOT NULL, language_filters_json TEXT NOT NULL
        );
        CREATE TABLE code_repository_files (
            repository_id TEXT NOT NULL, source_scope TEXT NOT NULL,
            file_id TEXT NOT NULL, path TEXT NOT NULL, language_id TEXT NOT NULL,
            blob_hash TEXT NOT NULL, byte_len INTEGER NOT NULL,
            line_count INTEGER NOT NULL, parse_status TEXT NOT NULL,
            degraded_reason TEXT,
            PRIMARY KEY (source_scope, path)
        );
        CREATE TABLE code_workspace_package_mappings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            set_id TEXT NOT NULL, package_name TEXT NOT NULL,
            ecosystem TEXT NOT NULL, repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL, workspace_format TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            UNIQUE (set_id, package_name, ecosystem)
        );
        CREATE TABLE code_repository_cross_edges (
            edge_id TEXT PRIMARY KEY, set_id TEXT NOT NULL,
            from_source_scope TEXT NOT NULL, from_repository_id TEXT NOT NULL,
            from_record_kind TEXT NOT NULL, from_record_id TEXT NOT NULL,
            to_source_scope TEXT, to_repository_id TEXT,
            to_record_kind TEXT NOT NULL, to_record_id TEXT,
            edge_kind TEXT NOT NULL, resolution_state TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL, confidence_tier TEXT NOT NULL,
            evidence_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL
        );
        CREATE TABLE code_repository_imports (
            repository_id TEXT NOT NULL, source_scope TEXT NOT NULL,
            import_id TEXT NOT NULL, file_id TEXT NOT NULL, path TEXT NOT NULL,
            module TEXT NOT NULL, target_hint TEXT, resolution_state TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL, confidence_tier TEXT NOT NULL,
            line_start INTEGER NOT NULL, line_end INTEGER NOT NULL,
            PRIMARY KEY (source_scope, import_id)
        );
        CREATE TABLE code_repository_set_overlay_status (
            set_id TEXT PRIMARY KEY, state TEXT NOT NULL,
            refreshed_at_ms INTEGER, edge_count INTEGER NOT NULL,
            member_versions_json TEXT NOT NULL, degraded_reason TEXT
        );",
    )
    .expect("schema");
    conn
}

fn workspace(format: CodeMonorepoWorkspaceFormat) -> CodeMonorepoWorkspace {
    CodeMonorepoWorkspace {
        format,
        root_path: "/repo".to_owned(),
        workspace_file_path: match format {
            CodeMonorepoWorkspaceFormat::Pnpm => "/repo/pnpm-workspace.yaml",
            CodeMonorepoWorkspaceFormat::GoModules => "/repo/go.work",
            CodeMonorepoWorkspaceFormat::CargoWorkspace => "/repo/Cargo.toml",
        }
        .to_owned(),
        members: vec![
            CodeWorkspaceMember {
                package_name: match format {
                    CodeMonorepoWorkspaceFormat::Pnpm => "@scope/core",
                    CodeMonorepoWorkspaceFormat::GoModules => "example.com/svc/api",
                    CodeMonorepoWorkspaceFormat::CargoWorkspace => "core",
                }
                .to_owned(),
                relative_path: "packages/core".to_owned(),
            },
            CodeWorkspaceMember {
                package_name: match format {
                    CodeMonorepoWorkspaceFormat::Pnpm => "@scope/app",
                    CodeMonorepoWorkspaceFormat::GoModules => "example.com/svc/app",
                    CodeMonorepoWorkspaceFormat::CargoWorkspace => "app",
                }
                .to_owned(),
                relative_path: "packages/app".to_owned(),
            },
        ],
    }
}

fn insert_scope(transaction: &Transaction<'_>, source_scope: &str, commit: &str) {
    transaction
        .execute(
            "INSERT INTO code_repository_scopes (
                source_scope, repository_id, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json
            )
            VALUES (?1, 'repo', ?2, 'tree', '[]', '[]')",
            params![source_scope, commit],
        )
        .expect("insert scope");
}

fn insert_unresolved_import(
    transaction: &Transaction<'_>,
    source_scope: &str,
    import_id: &str,
    module: &str,
) {
    insert_unresolved_import_with_language(
        transaction,
        source_scope,
        import_id,
        module,
        "file-main",
        "packages/app/src/main.ts",
        "typescript",
    );
}

fn insert_unresolved_import_with_language(
    transaction: &Transaction<'_>,
    source_scope: &str,
    import_id: &str,
    module: &str,
    file_id: &str,
    path: &str,
    language_id: &str,
) {
    insert_source_file(transaction, source_scope, file_id, path, language_id);
    transaction
        .execute(
            "INSERT INTO code_repository_imports (
                repository_id, source_scope, import_id, file_id, path, module,
                target_hint, resolution_state, confidence_basis_points,
                confidence_tier, line_start, line_end
            )
            VALUES ('repo', ?1, ?2, ?3, ?4, ?5, NULL, 'unresolved',
                    0, 'unresolved', 1, 1)",
            params![source_scope, import_id, file_id, path, module],
        )
        .expect("insert unresolved import");
}

fn insert_source_file(
    transaction: &Transaction<'_>,
    source_scope: &str,
    file_id: &str,
    path: &str,
    language_id: &str,
) {
    transaction
        .execute(
            "INSERT OR IGNORE INTO code_repository_files (
                repository_id, source_scope, file_id, path, language_id,
                blob_hash, byte_len, line_count, parse_status, degraded_reason
            )
            VALUES ('repo', ?1, ?2, ?3, ?4, 'hash', 1, 1, 'parsed', NULL)",
            params![source_scope, file_id, path, language_id],
        )
        .expect("insert source file");
}

#[test]
fn ecosystem_mappings_are_correct() {
    assert_eq!(
        ecosystem_for_format(CodeMonorepoWorkspaceFormat::Pnpm),
        "npm"
    );
    assert_eq!(
        ecosystem_for_format(CodeMonorepoWorkspaceFormat::GoModules),
        "go"
    );
    assert_eq!(
        ecosystem_for_format(CodeMonorepoWorkspaceFormat::CargoWorkspace),
        "rust"
    );
    assert_eq!(ecosystem_for_language("typescript"), Some("npm"));
    assert_eq!(ecosystem_for_language("tsx"), Some("npm"));
    assert_eq!(ecosystem_for_language("go"), Some("go"));
    assert_eq!(ecosystem_for_language("rust"), Some("rust"));
    assert_eq!(ecosystem_for_language("python"), None);
}

#[test]
fn format_keys_are_correct() {
    assert_eq!(
        workspace_format_key(CodeMonorepoWorkspaceFormat::Pnpm),
        "pnpm"
    );
    assert_eq!(
        workspace_format_key(CodeMonorepoWorkspaceFormat::GoModules),
        "go_modules"
    );
    assert_eq!(
        workspace_format_key(CodeMonorepoWorkspaceFormat::CargoWorkspace),
        "cargo_workspace"
    );
}

#[test]
fn package_candidates_preserve_path_and_namespace_prefixes() {
    assert_eq!(
        workspace_package_candidates("example.com/svc/api/client"),
        vec![
            "example.com/svc/api/client",
            "example.com/svc/api",
            "example.com/svc",
            "example.com",
            "example",
        ]
    );
    assert_eq!(
        workspace_package_candidates("@scope/core/utils"),
        vec!["@scope/core/utils", "@scope/core", "@scope"]
    );
    assert_eq!(
        workspace_package_candidates("serde::de::Deserialize"),
        vec!["serde::de::Deserialize", "serde::de", "serde"]
    );
    assert!(workspace_package_candidates("  ").is_empty());
}

#[test]
fn exact_package_lookup_binds_package_after_set_id() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    txn.execute(
        "INSERT INTO code_workspace_package_mappings
         (set_id, package_name, ecosystem, repository_id, source_scope,
          workspace_format, created_at_ms)
         VALUES ('set-1', '@scope/core', 'npm', 'repo', 'scope-core', 'pnpm', 1)",
        [],
    )
    .expect("insert mapping");

    let target = find_workspace_mapping_target(&txn, "set-1", "@scope/core", "npm")
        .expect("lookup should not fail")
        .expect("exact package should resolve");

    assert_eq!(target.package_name, "@scope/core");
    assert_eq!(target.source_scope, "scope-core");
    assert!(
        find_workspace_mapping_target(&txn, "set-1", "@scope/core", "rust")
            .expect("lookup should not fail")
            .is_none()
    );
}

#[test]
fn subpath_lookup_matches_longest_workspace_package_prefix() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    txn.execute(
        "INSERT INTO code_workspace_package_mappings
         (set_id, package_name, ecosystem, repository_id, source_scope,
          workspace_format, created_at_ms)
         VALUES
            ('set-1', 'example.com/svc', 'go', 'repo', 'scope-svc', 'go_modules', 1),
            ('set-1', 'example.com/svc/api', 'go', 'repo', 'scope-api', 'go_modules', 1)",
        [],
    )
    .expect("insert mappings");

    let target = find_workspace_mapping_target(&txn, "set-1", "example.com/svc/api/client", "go")
        .expect("lookup should not fail")
        .expect("subpath package should resolve");

    assert_eq!(target.package_name, "example.com/svc/api");
    assert_eq!(target.source_scope, "scope-api");
    assert_eq!(
        matches_workspace_package(&txn, "set-1", "example.com/svc/api/client", "go")
            .expect("package match should not fail"),
        Some("example.com/svc/api".to_owned())
    );
}

#[test]
fn empty_workspaces_noop() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    let result = resolve_workspace_imports(&txn, &[], "repo", "scope");
    assert!(result.is_ok());
}

#[test]
fn empty_workspaces_clear_previous_workspace_state() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import(&txn, "scope-main", "import-core", "@scope/core");
    insert_source_file(
        &txn,
        "scope-main",
        "file-core-package",
        "packages/core/package.json",
        "json",
    );

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");
    resolve_workspace_imports(&txn, &[], "repo", "scope-main")
        .expect("empty workspace result should clear state");

    for table in [
        "code_repository_sets",
        "code_repository_set_members",
        "code_workspace_package_mappings",
        "code_repository_cross_edges",
        "code_repository_set_overlay_status",
    ] {
        let count: u32 = txn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("table count");
        assert_eq!(count, 0, "{table} should be cleared");
    }
}

#[test]
fn resolve_workspace_imports_creates_edge_for_package_subpath() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import(&txn, "scope-main", "import-core-utils", "@scope/core/utils");
    insert_source_file(
        &txn,
        "scope-main",
        "file-core-package",
        "packages/core/package.json",
        "json",
    );

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");

    let edge = txn
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
fn resolve_workspace_imports_marks_overlay_fresh_even_without_edges() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");

    let status = txn
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
fn workspace_mappings_allow_same_package_name_across_ecosystems() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
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

    resolve_workspace_imports(&txn, &[pnpm, cargo], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let ecosystems: Vec<String> = {
        let mut statement = txn
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
fn workspace_member_paths_are_keyed_by_package_and_ecosystem() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import(&txn, "scope-main", "import-core", "core/utils");
    insert_source_file(
        &txn,
        "scope-main",
        "file-npm-package",
        "packages/core/package.json",
        "json",
    );
    insert_source_file(
        &txn,
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

    resolve_workspace_imports(&txn, &[pnpm, cargo], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let target_file: String = txn
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
fn auto_workspace_set_does_not_reuse_user_workspace_alias() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    txn.execute(
        "INSERT INTO code_repository_sets
         (set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms)
         VALUES ('user-set', 'repo-workspace', 'user managed', '{}', 1, 1)",
        [],
    )
    .expect("user set should insert");

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("auto workspace should not collide with user set alias");

    let aliases = {
        let mut statement = txn
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
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("auto workspace should resolve");

    let member_ref = txn
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
fn go_workspace_member_paths_strip_leading_current_directory() {
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
fn resolve_workspace_imports_skips_self_imports_inside_member_path() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &txn,
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

    resolve_workspace_imports(&txn, &[pnpm], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let edge_count: u32 = txn
        .query_row(
            "SELECT COUNT(*) FROM code_repository_cross_edges",
            [],
            |row| row.get(0),
        )
        .expect("edge count");
    assert_eq!(edge_count, 0);
}

#[test]
fn root_workspace_member_targets_ecosystem_manifest() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &txn,
        "scope-main",
        "import-root",
        "@scope/root",
        "file-app",
        "packages/app/src/main.ts",
        "typescript",
    );
    insert_source_file(&txn, "scope-main", "file-root-cargo", "Cargo.toml", "rust");
    insert_source_file(
        &txn,
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

    resolve_workspace_imports(&txn, &[pnpm], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let target_file: String = txn
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
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &txn,
        "scope-main",
        "import-api",
        "example.com/svc/api/client",
        "file-main",
        "cmd/app/main.go",
        "go",
    );
    insert_source_file(&txn, "scope-main", "file-root-mod", "go.mod", "go");
    insert_source_file(&txn, "scope-main", "file-api-mod", "api/go.mod", "go");
    let mut go = workspace(CodeMonorepoWorkspaceFormat::GoModules);
    go.members[0].relative_path = "./api".to_owned();

    resolve_workspace_imports(&txn, &[go], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let target_file: String = txn
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
fn go_workspace_lookup_strips_import_alias_tokens() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &txn,
        "scope-main",
        "import-api",
        "api example.com/svc/api/client",
        "file-main",
        "cmd/app/main.go",
        "go",
    );
    insert_source_file(&txn, "scope-main", "file-api-mod", "api/go.mod", "go");
    let mut go = workspace(CodeMonorepoWorkspaceFormat::GoModules);
    go.members[0].relative_path = "./api".to_owned();

    resolve_workspace_imports(&txn, &[go], "repo", "scope-main")
        .expect("workspace imports should resolve");

    let target_file: String = txn
        .query_row(
            "SELECT to_record_id FROM code_repository_cross_edges WHERE from_record_id = 'import-api'",
            [],
            |row| row.get(0),
        )
        .expect("edge should exist");
    assert_eq!(target_file, "file-api-mod");
}

#[test]
fn resolve_workspace_imports_skips_local_modules() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import(&txn, "scope-main", "import-local", "./core");

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");

    let edge_count: u32 = txn
        .query_row(
            "SELECT COUNT(*) FROM code_repository_cross_edges",
            [],
            |row| row.get(0),
        )
        .expect("edge count");
    assert_eq!(edge_count, 0);
}

#[test]
fn resolve_workspace_imports_skips_package_name_in_wrong_ecosystem() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    insert_unresolved_import_with_language(
        &txn,
        "scope-main",
        "import-rust-core",
        "@scope/core",
        "file-lib",
        "src/lib.rs",
        "rust",
    );
    insert_source_file(
        &txn,
        "scope-main",
        "file-core-package",
        "packages/core/package.json",
        "json",
    );

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-main",
    )
    .expect("workspace imports should resolve");

    let edge_count: u32 = txn
        .query_row(
            "SELECT COUNT(*) FROM code_repository_cross_edges",
            [],
            |row| row.get(0),
        )
        .expect("edge count");
    assert_eq!(edge_count, 0);
}

#[test]
fn resolve_workspace_imports_preserves_retained_scope_edges() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-old", "commit-old");
    insert_scope(&txn, "scope-new", "commit-new");
    insert_unresolved_import(&txn, "scope-old", "import-old", "@scope/core/old");
    insert_unresolved_import(&txn, "scope-new", "import-new", "@scope/core/new");

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-old",
    )
    .expect("old workspace imports should resolve");
    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-new",
    )
    .expect("new workspace imports should resolve");

    let mut member_stmt = txn
        .prepare(
            "SELECT source_scope, resolved_commit_sha
             FROM code_repository_set_members
             ORDER BY source_scope",
        )
        .expect("member statement");
    let members = member_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("members should load")
        .collect::<Result<Vec<_>, _>>()
        .expect("members should collect");
    let old_edge_count: u32 = txn
        .query_row(
            "SELECT COUNT(*) FROM code_repository_cross_edges
             WHERE from_source_scope = 'scope-old'",
            [],
            |row| row.get(0),
        )
        .expect("old edge count");
    let overlay_edge_count: u32 = txn
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
fn empty_workspaces_clear_only_current_retained_scope_state() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-old", "commit-old");
    insert_scope(&txn, "scope-new", "commit-new");
    insert_unresolved_import(&txn, "scope-old", "import-old", "@scope/core/old");
    insert_unresolved_import(&txn, "scope-new", "import-new", "@scope/core/new");

    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-old",
    )
    .expect("old workspace imports should resolve");
    resolve_workspace_imports(
        &txn,
        &[workspace(CodeMonorepoWorkspaceFormat::Pnpm)],
        "repo",
        "scope-new",
    )
    .expect("new workspace imports should resolve");
    resolve_workspace_imports(&txn, &[], "repo", "scope-new")
        .expect("empty workspace should clear only the current scope");

    let mut member_statement = txn
        .prepare("SELECT source_scope FROM code_repository_set_members ORDER BY source_scope")
        .expect("member statement");
    let remaining_members: Vec<String> = member_statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("members should load")
        .collect::<Result<Vec<_>, _>>()
        .expect("members should collect");
    let mut edge_statement = txn
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
    let overlay_edge_count: u32 = txn
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

#[test]
fn upsert_workspace_package_mappings_prunes_removed_members() {
    let mut conn = workspace_schema_connection();
    let txn = conn.transaction().expect("txn");
    insert_scope(&txn, "scope-main", "commit-main");
    let full_workspace = workspace(CodeMonorepoWorkspaceFormat::Pnpm);
    let mut reduced_workspace = workspace(CodeMonorepoWorkspaceFormat::Pnpm);
    reduced_workspace
        .members
        .retain(|member| member.package_name != "@scope/core");

    resolve_workspace_imports(&txn, &[full_workspace], "repo", "scope-main")
        .expect("full workspace imports should resolve");
    resolve_workspace_imports(&txn, &[reduced_workspace], "repo", "scope-main")
        .expect("reduced workspace imports should resolve");

    let removed_count: u32 = txn
        .query_row(
            "SELECT COUNT(*) FROM code_workspace_package_mappings
             WHERE package_name = '@scope/core'",
            [],
            |row| row.get(0),
        )
        .expect("removed mapping count");
    let remaining_count: u32 = txn
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

#[test]
fn is_local_or_relative_detects_correctly() {
    assert!(is_local_or_relative_module("./foo"));
    assert!(is_local_or_relative_module("../foo"));
    assert!(is_local_or_relative_module("crate::foo"));
    assert!(is_local_or_relative_module("self::foo"));
    assert!(is_local_or_relative_module("super::foo"));
    assert!(is_local_or_relative_module("crate"));
    assert!(is_local_or_relative_module("self"));
    assert!(is_local_or_relative_module("super"));
    assert!(is_local_or_relative_module(""));
    assert!(is_local_or_relative_module("  "));
    assert!(!is_local_or_relative_module("example.com/api"));
    assert!(!is_local_or_relative_module("@scope/pkg"));
}
