use rusqlite::params;

use super::*;
use crate::storage::{CodeRepositorySetMemberSeed, CodeRepositorySetSeed, SqliteGraphStore};

#[tokio::test]
async fn repository_set_overlay_refresh_resolves_pnpm_workspace_package_imports() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let (summary, edges) = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-app", "app", "scope-app", "tree-app")?;
            insert_repository_scope(connection, "repo-ui", "ui", "scope-ui", "tree-ui")?;
            insert_import(
                connection,
                "repo-app",
                "scope-app",
                "import-ui-components",
                "\"@myorg/ui-components\"",
            )?;
            insert_import(
                connection,
                "repo-app",
                "scope-app",
                "import-missing-package",
                "\"@myorg/missing\"",
            )?;
            insert_file(
                connection,
                "repo-ui",
                "scope-ui",
                "ui-package-file",
                "packages/ui/package.json",
                "json",
            )?;
            insert_file(
                connection,
                "repo-ui",
                "scope-ui",
                "ui-index-file",
                "packages/ui/src/index.ts",
                "typescript",
            )?;
            insert_symbol(
                connection,
                "repo-ui",
                "scope-ui",
                "ui-index-file",
                "ui-index-symbol",
                "packages/ui/src/index.ts",
                "UiComponents",
            )?;
            insert_chunk(
                connection,
                "repo-ui",
                "scope-ui",
                "pnpm-workspace",
                "pnpm-workspace.yaml",
                "packages:\n  - 'packages/*'\n",
            )?;
            let package_manifest = large_package_manifest_with_late_fields();
            insert_chunk(
                connection,
                "repo-ui",
                "scope-ui",
                "ui-package-json",
                "packages/ui/package.json",
                &package_manifest,
            )?;
            let set = code_set::create_set(connection, set_seed("workspace", 20))?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-app", "app", "scope-app", 10),
            )?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-ui", "ui", "scope-ui", 0),
            )?;
            let summary = code_set::refresh_overlay(connection, "workspace", 30)?;
            let edges = code_set::cross_edges_for_set(connection, &set.set_id)?;

            Ok((summary, edges))
        })
        .await
        .expect("overlay should refresh");

    assert_eq!(summary.edge_count, 2);
    assert_eq!(summary.resolved_edge_count, 1);
    assert_eq!(summary.unresolved_edge_count, 1);
    assert!(edges.iter().any(|edge| {
        edge.from_record_id == "import-ui-components"
            && edge.resolution_state == "resolved"
            && edge.to_source_scope.as_deref() == Some("scope-ui")
            && edge.to_record_kind == "code_file"
            && edge.to_record_id.as_deref() == Some("ui-index-file")
    }));
    assert!(edges.iter().any(|edge| {
        edge.from_record_id == "import-missing-package"
            && edge.resolution_state == "unresolved"
            && edge.to_source_scope.is_none()
            && edge.to_repository_id.is_none()
            && edge.to_record_kind == "unresolved_target"
            && edge.evidence_json.contains("@myorg/missing")
    }));
}

fn large_package_manifest_with_late_fields() -> String {
    format!(
        "{{\"private\":true,\"padding\":\"{}\",\"name\":\"@myorg/ui-components\",\"main\":\"src/index.ts\"}}",
        "x".repeat(8_192)
    )
}

fn set_seed(alias: &str, now_ms: u64) -> CodeRepositorySetSeed {
    CodeRepositorySetSeed {
        alias: alias.to_owned(),
        description: Some(format!("{alias} description")),
        default_ref_policy_json: "{\"default_ref\":\"HEAD\"}".to_owned(),
        now_ms,
    }
}

fn member_seed(
    set_alias: &str,
    repository_id: &str,
    repository_alias: &str,
    source_scope: &str,
    priority: i32,
) -> CodeRepositorySetMemberSeed {
    CodeRepositorySetMemberSeed {
        set_alias: set_alias.to_owned(),
        repository_id: repository_id.to_owned(),
        repository_alias: repository_alias.to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: format!("commit-{source_scope}"),
        source_scope: source_scope.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        priority,
    }
}

fn insert_repository_scope(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    alias: &str,
    source_scope: &str,
    tree_hash: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repositories (
            repository_id, alias, root_path, path_filters_json, language_filters_json,
            last_indexed_scope_id, last_indexed_commit, tree_hash, state,
            indexed_file_count, symbol_count, reference_count, chunk_count, stale,
            degraded_reason
        )
        VALUES (?1, ?2, '/tmp/repo', '[]', '[]',
                ?3, ?4, ?5, 'indexed', 1, 0, 0, 0, 0, NULL)
        ",
        params![
            repository_id,
            alias,
            source_scope,
            format!("commit-{source_scope}"),
            tree_hash,
        ],
    )?;
    connection.execute(
        "
        INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, '[]', '[]', 1, 0, 0, 0, 0, NULL)
        ",
        params![
            source_scope,
            repository_id,
            format!("commit-{source_scope}"),
            tree_hash,
        ],
    )?;
    Ok(())
}

fn insert_file(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    file_id: &str,
    path: &str,
    language_id: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_files (
            repository_id, source_scope, file_id, path, language_id, blob_hash,
            byte_len, line_count, parse_status, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'blob', 1, 1, 'parsed', NULL)
        ",
        params![repository_id, source_scope, file_id, path, language_id],
    )?;
    Ok(())
}

fn insert_symbol(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    file_id: &str,
    symbol_id: &str,
    path: &str,
    name: &str,
) -> Result<(), crate::storage::StorageError> {
    let qualified_name = name;
    connection.execute(
        "
        INSERT INTO code_repository_symbols (
            repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
            file_id, path, language_id, name, qualified_name, kind, signature,
            doc_comment, byte_start, byte_end, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'typescript', ?7, ?8,
                'function', ?9, NULL, 0, 1, 1, 1)
        ",
        params![
            repository_id,
            source_scope,
            symbol_id,
            format!("repo://{repository_id}/{path}::{qualified_name}"),
            file_id,
            path,
            name,
            qualified_name,
            format!("export function {name}() {{}}"),
        ],
    )?;
    Ok(())
}

fn insert_chunk(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    chunk_id: &str,
    path: &str,
    content: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_chunks (
            repository_id, source_scope, chunk_id, file_id, path, language_id, content,
            byte_start, byte_end, line_start, line_end, symbol_snapshot_id
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'unknown', ?6, 0, ?7, 1, 1, NULL)
        ",
        params![
            repository_id,
            source_scope,
            chunk_id,
            format!("file-{source_scope}"),
            path,
            content,
            content.len() as u32,
        ],
    )?;
    Ok(())
}

fn insert_import(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    import_id: &str,
    module: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_imports (
            repository_id, source_scope, import_id, file_id, path, module, target_hint,
            resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, 'src/client.ts', ?5, ?5, 'unresolved', 10000, 'extracted', 1, 1)
        ",
        params![
            repository_id,
            source_scope,
            import_id,
            format!("file-{source_scope}"),
            module,
        ],
    )?;
    Ok(())
}
