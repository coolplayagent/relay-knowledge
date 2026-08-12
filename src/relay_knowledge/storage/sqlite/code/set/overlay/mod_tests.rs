//! Regression tests for repository-set overlay refresh and resolution.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;

use super::super as set;
use super::super::tests::support::*;
use crate::storage::{
    CodeRepositorySetRefreshPublication, CodeRepositorySetRefreshTaskClaimRequest,
    CodeRepositorySetRefreshTaskSeed, SqliteGraphStore,
};

use super::super::capacity::{
    MAX_REPOSITORY_SET_OVERLAY_EDGES, MAX_REPOSITORY_SET_OVERLAY_IMPORTS,
};

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

#[tokio::test]
async fn repository_set_overlay_takeover_fences_the_stale_attempt_publication() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let now_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    let (first, takeover) = store
        .run(move |connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(
                connection,
                "repo-a",
                "app",
                "scope-a-new",
                "tree-a-new",
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
            insert_import(
                connection,
                "repo-a",
                "scope-a-new",
                "import-service-new",
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
            let repository_set = set::create_set(connection, set_seed("workspace", now_ms))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 10),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            let queued = set::refresh_tasks::queue_refresh_task(
                connection,
                CodeRepositorySetRefreshTaskSeed {
                    set_id: repository_set.set_id,
                    set_alias: String::from("workspace"),
                    input_fingerprint: String::from("takeover"),
                    now_ms,
                },
            )?;
            let first = set::refresh_tasks::claim_refresh_task(
                connection,
                CodeRepositorySetRefreshTaskClaimRequest {
                    task_id: Some(queued.task_id.clone()),
                    lease_owner: String::from("worker-old"),
                    lease_duration_ms: 0,
                    max_attempts: 3,
                    now_ms,
                },
            )?
            .expect("first attempt should claim");
            let takeover = set::refresh_tasks::claim_refresh_task(
                connection,
                CodeRepositorySetRefreshTaskClaimRequest {
                    task_id: Some(queued.task_id),
                    lease_owner: String::from("worker-new"),
                    lease_duration_ms: 60_000,
                    max_attempts: 3,
                    now_ms,
                },
            )?
            .expect("expired attempt should be taken over");
            Ok((first, takeover))
        })
        .await
        .expect("takeover fixture should persist");

    let published = store
        .run({
            let takeover = takeover.clone();
            move |connection| {
                set::refresh_overlay_for_task(
                    connection,
                    "workspace",
                    CodeRepositorySetRefreshPublication {
                        task_id: takeover.task_id,
                        set_id: takeover.set_id,
                        lease_owner: String::from("worker-new"),
                        attempt_count: takeover.attempt_count,
                        member_replacements: vec![member_seed(
                            "workspace",
                            "repo-a",
                            "app",
                            "scope-a-new",
                            10,
                        )],
                    },
                )
            }
        })
        .await
        .expect("takeover attempt should publish");
    let expected_edges = store
        .run({
            let set_id = published.set_id.clone();
            move |connection| set::cross_edges_for_set(connection, &set_id)
        })
        .await
        .expect("published edges should load");

    let stale_error = store
        .run(move |connection| {
            set::refresh_overlay_for_task(
                connection,
                "workspace",
                CodeRepositorySetRefreshPublication {
                    task_id: first.task_id,
                    set_id: first.set_id,
                    lease_owner: String::from("worker-old"),
                    attempt_count: first.attempt_count,
                    member_replacements: vec![member_seed(
                        "workspace",
                        "repo-a",
                        "app",
                        "scope-a",
                        10,
                    )],
                },
            )
        })
        .await
        .expect_err("superseded attempt must not publish");
    assert!(
        stale_error
            .to_string()
            .contains("lease is no longer active")
    );

    let actual_edges = store
        .run({
            let set_id = published.set_id;
            move |connection| set::cross_edges_for_set(connection, &set_id)
        })
        .await
        .expect("overlay should remain readable");
    assert_eq!(actual_edges, expected_edges);
    assert_eq!(actual_edges.len(), 1);
    assert_eq!(actual_edges[0].resolution_state, "resolved");
    assert_eq!(actual_edges[0].from_source_scope, "scope-a-new");
    let status = store
        .run(|connection| set::set_status(connection, "workspace"))
        .await
        .expect("repository-set status should load")
        .expect("repository set should remain registered");
    assert_eq!(status.overlay.state, "fresh");
    assert_eq!(status.overlay.edge_count, 1);
    assert_eq!(
        status
            .members
            .iter()
            .find(|member| member.member.repository_id == "repo-a")
            .map(|member| member.member.source_scope.as_str()),
        Some("scope-a-new")
    );
}

#[tokio::test]
async fn repository_set_overlay_rejects_import_cap_plus_one_without_marking_fresh() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let error = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            set::create_set(connection, set_seed("workspace", 10))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare(
                    "INSERT INTO code_repository_imports (
                         repository_id, source_scope, import_id, file_id, path, module,
                         target_hint, resolution_state, confidence_basis_points,
                         confidence_tier, line_start, line_end
                     ) VALUES ('repo-a', 'scope-a', ?1, 'file-scope-a', 'src/app.rs',
                               'external.module', 'external.module', 'unresolved', 10000,
                               'extracted', 1, 1)",
                )?;
                for index in 0..=MAX_REPOSITORY_SET_OVERLAY_IMPORTS {
                    statement.execute(params![format!("import-{index:05}")])?;
                }
            }
            transaction.commit()?;
            set::refresh_overlay(connection, "workspace", 20)
        })
        .await
        .expect_err("import cap plus one must reject overlay refresh");

    assert!(matches!(
        error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    let status = store
        .run(|connection| set::set_status(connection, "workspace"))
        .await
        .expect("status should query")
        .expect("set should exist");
    assert_eq!(status.overlay.state, "missing");
}

#[tokio::test]
async fn legacy_oversized_overlay_is_rejected_before_membership_delete() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let error = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            insert_repository_scope(connection, "repo-b", "svc", "scope-b", "tree-b", false)?;
            let repository_set = set::create_set(connection, set_seed("workspace", 10))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare(
                    "INSERT INTO code_repository_cross_edges (
                         edge_id, set_id, from_source_scope, from_repository_id,
                         from_record_kind, from_record_id, to_source_scope, to_repository_id,
                         to_record_kind, to_record_id, edge_kind, resolution_state,
                         confidence_basis_points, confidence_tier, evidence_json, created_at_ms
                     ) VALUES (?1, ?2, 'scope-a', 'repo-a', 'module_reference', ?1,
                               'scope-b', 'repo-b', 'code_symbol_snapshot', 'target', 'imports',
                               'resolved', 10000, 'explicit', '{}', 20)",
                )?;
                for index in 0..=MAX_REPOSITORY_SET_OVERLAY_EDGES {
                    statement.execute(params![
                        format!("legacy-edge-{index:05}"),
                        repository_set.set_id,
                    ])?;
                }
            }
            transaction.commit()?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )
        })
        .await
        .expect_err("legacy oversized overlay must require explicit maintenance");

    assert!(matches!(
        error,
        crate::storage::StorageError::CapacityExceeded(_)
    ));
    let (member_count, edge_count) = store
        .run(|connection| {
            let member_count = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_set_members",
                [],
                |row| row.get::<_, usize>(0),
            )?;
            let edge_count = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_cross_edges",
                [],
                |row| row.get::<_, usize>(0),
            )?;
            Ok((member_count, edge_count))
        })
        .await
        .expect("legacy counts should query");
    assert_eq!(member_count, 1);
    assert_eq!(edge_count, MAX_REPOSITORY_SET_OVERLAY_EDGES + 1);
}
