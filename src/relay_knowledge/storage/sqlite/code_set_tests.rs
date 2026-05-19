use rusqlite::params;

use super::*;
use crate::{
    domain::CodeRepositorySetOverlayStatus,
    storage::{CodeRepositorySetMemberSeed, CodeRepositorySetSeed, SqliteGraphStore},
};

#[tokio::test]
async fn repository_set_members_validate_real_indexed_scopes_and_report_missing_overlay() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            code_set::create_set(connection, set_seed("workspace", 10))?;
            Ok(())
        })
        .await
        .expect("fixture should insert");

    let unknown_set = store
        .run(|connection| {
            code_set::add_member(
                connection,
                member_seed("missing", "repo-a", "app", "scope-a", 0),
            )
        })
        .await
        .expect_err("unknown set should fail");
    assert!(unknown_set.to_string().contains("is not registered"));

    let unknown_scope = store
        .run(|connection| {
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-missing", 0),
            )
        })
        .await
        .expect_err("unknown scope should fail");
    assert!(unknown_scope.to_string().contains("is not indexed"));

    let wrong_repository = store
        .run(|connection| {
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-b", "other", "scope-a", 0),
            )
        })
        .await
        .expect_err("wrong repository should fail");
    assert!(
        wrong_repository
            .to_string()
            .contains("belongs to repository")
    );

    let member = store
        .run(|connection| {
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 5),
            )
        })
        .await
        .expect("member should add");
    assert_eq!(member.repository_alias, "app");
    assert_eq!(member.path_filters, ["src"]);

    let status = store
        .run(|connection| code_set::set_status(connection, "workspace"))
        .await
        .expect("status should query")
        .expect("set should exist");
    assert_eq!(status.members.len(), 1);
    assert_eq!(
        status.overlay,
        CodeRepositorySetOverlayStatus {
            state: "missing".to_owned(),
            stale: true,
            edge_count: 0,
            refreshed_at_ms: None,
            degraded_reason: None,
        }
    );
    assert_eq!(status.freshness_state, "overlay_stale");
}

#[tokio::test]
async fn repository_set_readding_repository_replaces_previous_member_snapshot() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let status = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(
                connection,
                "repo-a",
                "app",
                "scope-a-new",
                "tree-a-new",
                false,
            )?;
            code_set::create_set(connection, set_seed("workspace", 10))?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a-new", 9),
            )?;
            code_set::set_status(connection, "workspace")
        })
        .await
        .expect("status should query")
        .expect("set should exist");

    assert_eq!(status.members.len(), 1);
    assert_eq!(status.members[0].member.source_scope, "scope-a-new");
    assert_eq!(status.members[0].member.priority, 9);
}

#[tokio::test]
async fn repository_set_member_replacement_invalidates_stale_overlay_edges() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let (set_id, visible_edges) = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(
                connection,
                "repo-a",
                "app",
                "scope-a-new",
                "tree-new",
                false,
            )?;
            insert_repository_scope(connection, "repo-b", "svc", "scope-b", "tree-b", false)?;
            insert_import(
                connection,
                "repo-a",
                "scope-a",
                "import-service",
                "service::serve",
            )?;
            insert_symbol(
                connection,
                "repo-b",
                "scope-b",
                "serve-symbol",
                "serve",
                "service::serve",
            )?;
            let set = code_set::create_set(connection, set_seed("workspace", 10))?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            code_set::refresh_overlay(connection, "workspace", 20)?;
            assert_eq!(
                code_set::cross_edges_for_set(connection, &set.set_id)?.len(),
                1
            );
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a-new", 0),
            )?;
            insert_cross_edge(connection, &set.set_id, "scope-a", "scope-b")?;
            let visible_edges = code_set::cross_edges_for_set(connection, &set.set_id)?;
            Ok((set.set_id, visible_edges))
        })
        .await
        .expect("fixture should refresh and replace");

    assert!(visible_edges.is_empty());
    let status = store
        .run(move |connection| code_set::set_status(connection, "workspace"))
        .await
        .expect("status should query")
        .expect("set should exist");
    assert_eq!(status.repository_set.set_id, set_id);
    assert_eq!(status.overlay.state, "missing");
    assert!(status.overlay.stale);
}

#[tokio::test]
async fn repository_set_overlay_refresh_classifies_resolved_ambiguous_and_unresolved_edges() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let summary = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(connection, "repo-b", "svc", "scope-b", "tree-b", false)?;
            insert_repository_scope(connection, "repo-c", "lib", "scope-c", "tree-c", false)?;
            insert_import(
                connection,
                "repo-a",
                "scope-a",
                "import-resolved",
                "service::serve",
            )?;
            insert_import(
                connection,
                "repo-a",
                "scope-a",
                "import-ambiguous",
                "shared",
            )?;
            insert_import(
                connection,
                "repo-a",
                "scope-a",
                "import-unresolved",
                "missing",
            )?;
            insert_symbol(
                connection,
                "repo-b",
                "scope-b",
                "serve-symbol",
                "serve",
                "service::serve",
            )?;
            insert_symbol(
                connection,
                "repo-b",
                "scope-b",
                "shared-b",
                "shared",
                "service::shared",
            )?;
            insert_symbol(
                connection,
                "repo-c",
                "scope-c",
                "shared-c",
                "shared",
                "lib::shared",
            )?;
            code_set::create_set(connection, set_seed("workspace", 20))?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 10),
            )?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-c", "lib", "scope-c", 0),
            )?;
            code_set::refresh_overlay(connection, "workspace", 30)
        })
        .await
        .expect("overlay should refresh");

    assert_eq!(summary.edge_count, 3);
    assert_eq!(summary.resolved_edge_count, 1);
    assert_eq!(summary.ambiguous_edge_count, 1);
    assert_eq!(summary.unresolved_edge_count, 1);

    let edges = store
        .run({
            let set_id = summary.set_id.clone();
            move |connection| code_set::cross_edges_for_set(connection, &set_id)
        })
        .await
        .expect("edges should query");
    assert_eq!(edges.len(), 3);
    assert!(edges.iter().any(|edge| {
        edge.from_record_id == "import-resolved"
            && edge.resolution_state == "resolved"
            && edge.to_record_id.as_deref() == Some("serve-symbol")
    }));
    assert!(edges.iter().any(|edge| {
        edge.from_record_id == "import-ambiguous"
            && edge.resolution_state == "ambiguous"
            && edge.to_record_id.is_none()
    }));
    assert!(edges.iter().any(|edge| {
        edge.from_record_id == "import-unresolved"
            && edge.resolution_state == "unresolved"
            && edge.to_record_kind == "unresolved_target"
    }));

    store
        .run(|connection| {
            connection.execute(
                "UPDATE code_repository_scopes SET tree_hash = 'tree-a-new' WHERE source_scope = 'scope-a'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("scope version should change");
    let stale = store
        .run(|connection| code_set::set_status(connection, "workspace"))
        .await
        .expect("status should query")
        .expect("set should exist");
    assert_eq!(stale.overlay.state, "overlay_stale");
    assert!(stale.overlay.stale);
}

#[tokio::test]
async fn repository_set_overlay_refresh_ignores_local_import_basename_matches() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let summary = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(connection, "repo-b", "svc", "scope-b", "tree-b", false)?;
            insert_import(
                connection,
                "repo-a",
                "scope-a",
                "import-local",
                "crate::db::Pool",
            )?;
            insert_symbol(
                connection,
                "repo-b",
                "scope-b",
                "pool-symbol",
                "Pool",
                "service::Pool",
            )?;
            code_set::create_set(connection, set_seed("workspace", 20))?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            code_set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            code_set::refresh_overlay(connection, "workspace", 30)
        })
        .await
        .expect("overlay should refresh");

    assert_eq!(summary.edge_count, 1);
    assert_eq!(summary.resolved_edge_count, 0);
    assert_eq!(summary.unresolved_edge_count, 1);
}

#[tokio::test]
async fn repository_set_alias_lookup_does_not_match_existing_set_ids() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let (first, colliding) = store
        .run(|connection| {
            let first = code_set::create_set(connection, set_seed("workspace", 10))?;
            let colliding = code_set::create_set(connection, set_seed(first.set_id.as_str(), 20))?;
            Ok((first, colliding))
        })
        .await
        .expect("sets should create");

    let first_status = store
        .run(|connection| code_set::set_status(connection, "workspace"))
        .await
        .expect("status should query")
        .expect("first set should exist");
    let colliding_alias = first.set_id.clone();
    let colliding_status = store
        .run(move |connection| code_set::set_status(connection, colliding_alias.as_str()))
        .await
        .expect("status should query")
        .expect("colliding alias should exist");

    assert_eq!(first_status.repository_set.set_id, first.set_id);
    assert_eq!(colliding_status.repository_set.set_id, colliding.set_id);
    assert_eq!(colliding_status.repository_set.alias, first.set_id);
}

#[tokio::test]
async fn repository_set_overlay_refresh_rejects_empty_sets() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let error = store
        .run(|connection| {
            code_set::create_set(connection, set_seed("workspace", 10))?;
            code_set::refresh_overlay(connection, "workspace", 20)
        })
        .await
        .expect_err("empty set should fail");

    assert!(error.to_string().contains("has no members"));
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
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        priority,
    }
}

fn insert_repository_scope(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    alias: &str,
    source_scope: &str,
    tree_hash: &str,
    stale: bool,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT OR IGNORE INTO code_repositories (
            repository_id, alias, root_path, path_filters_json, language_filters_json,
            last_indexed_scope_id, last_indexed_commit, tree_hash, state,
            indexed_file_count, symbol_count, reference_count, chunk_count, stale,
            degraded_reason
        )
        VALUES (?1, ?2, '/tmp/repo', '[\"src\"]', '[\"rust\"]',
                ?3, ?4, ?5, 'indexed', 1, 1, 0, 0, ?6, NULL)
        ",
        params![
            repository_id,
            alias,
            source_scope,
            format!("commit-{source_scope}"),
            tree_hash,
            i64::from(stale),
        ],
    )?;
    connection.execute(
        "
        INSERT INTO code_repository_scopes (
            source_scope, repository_id, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, indexed_file_count,
            symbol_count, reference_count, chunk_count, stale, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, '[\"src\"]', '[\"rust\"]', 1, 1, 0, 0, ?5, NULL)
        ",
        params![
            source_scope,
            repository_id,
            format!("commit-{source_scope}"),
            tree_hash,
            i64::from(stale),
        ],
    )?;
    connection.execute(
        "
        INSERT INTO code_repository_files (
            repository_id, source_scope, file_id, path, language_id, blob_hash,
            byte_len, line_count, parse_status, degraded_reason
        )
        VALUES (?1, ?2, ?3, ?4, 'rust', 'blob', 1, 1, 'parsed', NULL)
        ",
        params![
            repository_id,
            source_scope,
            format!("file-{source_scope}"),
            format!("src/{alias}.rs"),
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
        VALUES (?1, ?2, ?3, ?4, 'src/client.rs', ?5, ?5, 'unresolved', 10000, 'extracted', 1, 1)
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

fn insert_symbol(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    source_scope: &str,
    symbol_id: &str,
    name: &str,
    qualified_name: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_symbols (
            repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
            file_id, path, language_id, name, qualified_name, kind, signature,
            doc_comment, byte_start, byte_end, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'src/service.rs', 'rust', ?6, ?7,
                'function', 'fn target()', NULL, 0, 10, 1, 1)
        ",
        params![
            repository_id,
            source_scope,
            symbol_id,
            format!("{repository_id}::{qualified_name}"),
            format!("file-{source_scope}"),
            name,
            qualified_name,
        ],
    )?;
    Ok(())
}

fn insert_cross_edge(
    connection: &mut rusqlite::Connection,
    set_id: &str,
    from_source_scope: &str,
    to_source_scope: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "
        INSERT INTO code_repository_cross_edges (
            edge_id, set_id, from_source_scope, from_repository_id, from_record_kind,
            from_record_id, to_source_scope, to_repository_id, to_record_kind, to_record_id,
            edge_kind, resolution_state, confidence_basis_points, confidence_tier,
            evidence_json, created_at_ms
        )
        VALUES ('stale-edge', ?1, ?2, 'repo-a', 'module_reference',
                'import-service', ?3, 'repo-b', 'code_symbol_snapshot', 'serve-symbol',
                'imports', 'resolved', 10000, 'explicit', '{}', 40)
        ",
        params![set_id, from_source_scope, to_source_scope],
    )?;
    Ok(())
}
