//! Regression tests for repository-set overlay refresh and resolution.

use super::super as set;
use super::super::tests::support::*;
use crate::storage::SqliteGraphStore;

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
            set::create_set(connection, set_seed("workspace", 20))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 10),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-c", "lib", "scope-c", 0),
            )?;
            set::refresh_overlay(connection, "workspace", 30)
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
            move |connection| set::cross_edges_for_set(connection, &set_id)
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
        .run(|connection| set::set_status(connection, "workspace"))
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
            set::create_set(connection, set_seed("workspace", 20))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            set::refresh_overlay(connection, "workspace", 30)
        })
        .await
        .expect("overlay should refresh");

    assert_eq!(summary.edge_count, 0);
    assert_eq!(summary.resolved_edge_count, 0);
    assert_eq!(summary.unresolved_edge_count, 0);
}

#[tokio::test]
async fn repository_set_overlay_refresh_skips_locally_resolved_imports() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let summary = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(connection, "repo-b", "svc", "scope-b", "tree-b", false)?;
            insert_import_with_state(
                connection,
                "repo-a",
                "scope-a",
                "import-local-resolved",
                "app.RetryPolicy",
                "resolved",
            )?;
            insert_symbol(
                connection,
                "repo-b",
                "scope-b",
                "retry-symbol",
                "RetryPolicy",
                "app::RetryPolicy",
            )?;
            set::create_set(connection, set_seed("workspace", 20))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            set::refresh_overlay(connection, "workspace", 30)
        })
        .await
        .expect("overlay should refresh");

    assert_eq!(summary.edge_count, 0);
    assert_eq!(summary.resolved_edge_count, 0);
}

#[tokio::test]
async fn repository_set_overlay_refresh_requires_full_module_match_for_external_imports() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let summary = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(connection, "repo-b", "svc", "scope-b", "tree-b", false)?;
            insert_import(
                connection,
                "repo-a",
                "scope-a",
                "import-external",
                "java.time.Duration",
            )?;
            insert_symbol(
                connection,
                "repo-b",
                "scope-b",
                "duration-symbol",
                "Duration",
                "service::Duration",
            )?;
            set::create_set(connection, set_seed("workspace", 20))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            set::refresh_overlay(connection, "workspace", 30)
        })
        .await
        .expect("overlay should refresh");

    assert_eq!(summary.edge_count, 1);
    assert_eq!(summary.resolved_edge_count, 0);
    assert_eq!(summary.unresolved_edge_count, 1);
}

#[tokio::test]
async fn repository_set_overlay_refresh_matches_go_module_manifest_prefixes() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let summary = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(connection, "repo-b", "sdk", "scope-b", "tree-b", false)?;
            insert_import(
                connection,
                "repo-a",
                "scope-a",
                "import-go-client",
                "\"go.temporal.io/sdk/client\"",
            )?;
            insert_file(
                connection,
                "repo-b",
                "scope-b",
                "sdk-client-file",
                "client/client.go",
                "go",
            )?;
            insert_chunk(
                connection,
                "repo-b",
                "scope-b",
                "go-mod-chunk",
                "go.mod",
                "module go.temporal.io/sdk\n",
            )?;
            set::create_set(connection, set_seed("workspace", 20))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "sdk", "scope-b", 0),
            )?;
            set::refresh_overlay(connection, "workspace", 30)
        })
        .await
        .expect("overlay should refresh");

    assert_eq!(summary.edge_count, 1);
    assert_eq!(summary.resolved_edge_count, 1);

    let edges = store
        .run({
            let set_id = summary.set_id.clone();
            move |connection| set::cross_edges_for_set(connection, &set_id)
        })
        .await
        .expect("edges should query");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].resolution_state, "resolved");
    assert_eq!(edges[0].to_record_kind, "code_file");
    assert_eq!(edges[0].to_record_id.as_deref(), Some("sdk-client-file"));
}

#[tokio::test]
async fn repository_set_overlay_refresh_matches_nested_go_module_manifest_prefixes() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let summary = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(connection, "repo-b", "otel", "scope-b", "tree-b", false)?;
            insert_import(
                connection,
                "repo-a",
                "scope-a",
                "import-component",
                "\"go.opentelemetry.io/collector/component\"",
            )?;
            insert_file(
                connection,
                "repo-b",
                "scope-b",
                "component-go-mod",
                "component/go.mod",
                "unknown",
            )?;
            insert_file(
                connection,
                "repo-b",
                "scope-b",
                "component-file",
                "component/identifiable.go",
                "go",
            )?;
            insert_chunk(
                connection,
                "repo-b",
                "scope-b",
                "component-go-mod-chunk",
                "component/go.mod",
                "module go.opentelemetry.io/collector/component\n",
            )?;
            set::create_set(connection, set_seed("workspace", 20))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "otel", "scope-b", 0),
            )?;
            set::refresh_overlay(connection, "workspace", 30)
        })
        .await
        .expect("overlay should refresh");

    assert_eq!(summary.edge_count, 1);
    assert_eq!(summary.resolved_edge_count, 1);

    let edges = store
        .run({
            let set_id = summary.set_id.clone();
            move |connection| set::cross_edges_for_set(connection, &set_id)
        })
        .await
        .expect("edges should query");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].resolution_state, "resolved");
    assert_eq!(edges[0].to_record_id.as_deref(), Some("component-file"));
}

#[tokio::test]
async fn repository_set_overlay_refresh_rejects_empty_sets() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let error = store
        .run(|connection| {
            set::create_set(connection, set_seed("workspace", 10))?;
            set::refresh_overlay(connection, "workspace", 20)
        })
        .await
        .expect_err("empty set should fail");

    assert!(error.to_string().contains("has no members"));
}
