use rusqlite::{Connection, Transaction, params};

use crate::domain::{CodeMonorepoWorkspace, CodeMonorepoWorkspaceFormat, CodeWorkspaceMember};

pub(super) fn workspace_schema_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("in-memory connection");
    connection
        .execute_batch(
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
    connection
}

pub(super) fn workspace(format: CodeMonorepoWorkspaceFormat) -> CodeMonorepoWorkspace {
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

pub(super) fn insert_scope(transaction: &Transaction<'_>, source_scope: &str, commit: &str) {
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

pub(super) fn insert_unresolved_import(
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

pub(super) fn insert_unresolved_import_with_language(
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

pub(super) fn insert_source_file(
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
