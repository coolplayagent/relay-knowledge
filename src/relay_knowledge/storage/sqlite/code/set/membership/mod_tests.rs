//! Regression tests for repository-set membership persistence.

use super::super as set;
use super::super::tests::support::*;
use crate::{domain::CodeRepositorySetOverlayStatus, storage::SqliteGraphStore};

#[tokio::test]
async fn repository_set_members_validate_real_indexed_scopes_and_report_missing_overlay() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
            set::create_set(connection, set_seed("workspace", 10))?;
            Ok(())
        })
        .await
        .expect("fixture should insert");

    let unknown_set = store
        .run(|connection| {
            set::add_member(
                connection,
                member_seed("missing", "repo-a", "app", "scope-a", 0),
            )
        })
        .await
        .expect_err("unknown set should fail");
    assert!(unknown_set.to_string().contains("is not registered"));

    let unknown_scope = store
        .run(|connection| {
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-missing", 0),
            )
        })
        .await
        .expect_err("unknown scope should fail");
    assert!(unknown_scope.to_string().contains("is not indexed"));

    let wrong_repository = store
        .run(|connection| {
            set::add_member(
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
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 5),
            )
        })
        .await
        .expect("member should add");
    assert_eq!(member.repository_alias, "app");
    assert_eq!(member.path_filters, ["src"]);

    let status = store
        .run(|connection| set::set_status(connection, "workspace"))
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
async fn repository_set_status_preserves_member_filters_when_scope_is_broader() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .run(|connection| {
            insert_repository_scope_with_filters(
                connection,
                "repo-a",
                "app",
                "scope-a",
                "tree-a",
                false,
                ("[]", "[]"),
            )?;
            set::create_set(connection, set_seed("workspace", 10))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 5),
            )?;
            Ok(())
        })
        .await
        .expect("fixture should insert");

    let status = store
        .run(|connection| set::set_status(connection, "workspace"))
        .await
        .expect("status should query")
        .expect("set should exist");

    let member = &status.members[0];
    assert_eq!(member.member.path_filters, ["src"]);
    assert_eq!(member.member.language_filters, ["rust"]);
    assert!(member.indexed_path_filters.is_empty());
    assert!(member.indexed_language_filters.is_empty());
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
            set::create_set(connection, set_seed("workspace", 10))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a-new", 9),
            )?;
            set::set_status(connection, "workspace")
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
            let set = set::create_set(connection, set_seed("workspace", 10))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            set::refresh_overlay(connection, "workspace", 20)?;
            assert_eq!(set::cross_edges_for_set(connection, &set.set_id)?.len(), 1);
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a-new", 0),
            )?;
            insert_cross_edge(connection, &set.set_id, "scope-a", "scope-b")?;
            let visible_edges = set::cross_edges_for_set(connection, &set.set_id)?;
            Ok((set.set_id, visible_edges))
        })
        .await
        .expect("fixture should refresh and replace");

    assert!(visible_edges.is_empty());
    let status = store
        .run(move |connection| set::set_status(connection, "workspace"))
        .await
        .expect("status should query")
        .expect("set should exist");
    assert_eq!(status.repository_set.set_id, set_id);
    assert_eq!(status.overlay.state, "missing");
    assert!(status.overlay.stale);
}

#[tokio::test]
async fn repository_set_member_removal_releases_scope_and_invalidates_overlay() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let (removed, status) = store
        .run(|connection| {
            insert_repository_scope(connection, "repo-a", "app", "scope-a", "tree-a", false)?;
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
            set::create_set(connection, set_seed("workspace", 10))?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-a", "app", "scope-a", 0),
            )?;
            set::add_member(
                connection,
                member_seed("workspace", "repo-b", "svc", "scope-b", 0),
            )?;
            set::refresh_overlay(connection, "workspace", 20)?;
            let removed = set::remove_member(connection, "workspace", "app")?;
            let status = set::set_status(connection, "workspace")?.expect("set should exist");
            Ok((removed, status))
        })
        .await
        .expect("member should remove");

    assert_eq!(removed.repository_alias, "app");
    assert_eq!(status.members.len(), 1);
    assert_eq!(status.members[0].member.repository_alias, "svc");
    assert_eq!(status.overlay.state, "missing");
    assert!(status.overlay.stale);
}

#[tokio::test]
async fn repository_set_alias_lookup_does_not_match_existing_set_ids() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let (first, colliding) = store
        .run(|connection| {
            let first = set::create_set(connection, set_seed("workspace", 10))?;
            let colliding = set::create_set(connection, set_seed(first.set_id.as_str(), 20))?;
            Ok((first, colliding))
        })
        .await
        .expect("sets should create");

    let first_status = store
        .run(|connection| set::set_status(connection, "workspace"))
        .await
        .expect("status should query")
        .expect("first set should exist");
    let colliding_alias = first.set_id.clone();
    let colliding_status = store
        .run(move |connection| set::set_status(connection, colliding_alias.as_str()))
        .await
        .expect("status should query")
        .expect("colliding alias should exist");

    assert_eq!(first_status.repository_set.set_id, first.set_id);
    assert_eq!(colliding_status.repository_set.set_id, colliding.set_id);
    assert_eq!(colliding_status.repository_set.alias, first.set_id);
}
