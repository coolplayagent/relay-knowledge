//! Direct routing tests for partitioned post-index maintenance.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::{
        CodeIndexResourceBudget, CodeIndexSession, CodeIndexSnapshot, CodeRepositoryRegistration,
    },
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
    storage::{CodeRepositoryStore, SqliteGraphStore, StorageError},
};

use super::PartitionedSqliteKnowledgeStore;

#[tokio::test]
async fn post_maintenance_routes_active_and_checkpoint_scopes_to_their_repository_shards() {
    let store = partitioned_store();
    for (repository_id, alias) in [("repo-active", "active"), ("repo-retained", "retained")] {
        store
            .upsert_code_repository(
                CodeRepositoryRegistration::new(
                    repository_id,
                    alias,
                    format!("/tmp/{repository_id}"),
                    Vec::new(),
                    Vec::new(),
                )
                .expect("registration should validate"),
            )
            .await
            .expect("repository should register");
    }
    super::indexing::lifecycle::seed_snapshot_for_test(
        &store,
        snapshot(
            "repo-active",
            "scope-active",
            "commit-active",
            "tree-active",
        ),
    )
    .await
    .expect("active repository scope should publish");
    super::indexing::lifecycle::seed_snapshot_for_test(
        &store,
        snapshot(
            "repo-retained",
            "scope-retained-active",
            "commit-retained-active",
            "tree-retained-active",
        ),
    )
    .await
    .expect("retained repository should have an active shard");
    super::indexing::lifecycle::seed_session_for_test(
        &store,
        CodeIndexSession {
            repository_id: "repo-retained".to_owned(),
            source_scope: "scope-retained-checkpoint".to_owned(),
            base_resolved_commit_sha: None,
            resolved_commit_sha: "commit-retained-checkpoint".to_owned(),
            tree_hash: "tree-retained-checkpoint".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
            full_replace: true,
            total_path_count: 0,
            changed_path_count: 0,
            skipped_unchanged_count: 0,
            deleted_paths: Vec::new(),
            changed_paths: Vec::new(),
            tombstones: Vec::new(),
            workspaces: Vec::new(),
            resource_budget: CodeIndexResourceBudget::default(),
        },
    )
    .await
    .expect("retained checkpoint should stage in its repository shard");
    assert_eq!(
        store
            .catalog
            .active_repository_for_scope("scope-active".to_owned())
            .await
            .expect("active route should load")
            .as_deref(),
        Some("repo-active")
    );
    assert_eq!(
        store
            .catalog
            .repository_for_scope("scope-retained-checkpoint".to_owned())
            .await
            .expect("retained route should load")
            .as_deref(),
        Some("repo-retained")
    );
    assert!(
        store
            .catalog
            .active_repository_for_scope("scope-retained-checkpoint".to_owned())
            .await
            .expect("retained active route should load")
            .is_none()
    );
    let active_shard = store
        .catalog
        .existing_repository_store("repo-active".to_owned())
        .await
        .expect("active repository shard should load")
        .expect("active repository shard should exist");
    let retained_shard = store
        .catalog
        .checkpoint_repository_store("repo-retained".to_owned())
        .await
        .expect("retained checkpoint shard should load")
        .expect("retained checkpoint shard should exist");
    set_maintenance_marker(&active_shard, 11).await;
    set_maintenance_marker(&retained_shard, 22).await;

    store
        .run_code_index_post_maintenance("repo-active".to_owned(), "scope-active".to_owned())
        .await
        .expect("active-scope maintenance should route");
    assert_ne!(maintenance_marker(&active_shard).await, 11);
    assert_eq!(maintenance_marker(&retained_shard).await, 22);

    store
        .run_code_index_post_maintenance(
            "repo-retained".to_owned(),
            "scope-retained-checkpoint".to_owned(),
        )
        .await
        .expect("retained-scope maintenance should route");
    assert_ne!(maintenance_marker(&retained_shard).await, 22);
}

fn partitioned_store() -> PartitionedSqliteKnowledgeStore {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-partitioned-maintenance-{}-{}",
        std::process::id(),
        now_millis()
    ));
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(
            "RELAY_KNOWLEDGE_HOME",
            root.to_str().expect("path is UTF-8"),
        )],
    )
    .expect("environment should parse");
    let paths = RuntimePaths::resolve(&environment.platform, &environment.paths)
        .expect("runtime paths should resolve");
    PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths)
        .expect("partitioned store should open")
}

fn snapshot(
    repository_id: &str,
    source_scope: &str,
    resolved_commit_sha: &str,
    tree_hash: &str,
) -> CodeIndexSnapshot {
    CodeIndexSnapshot {
        repository_id: repository_id.to_owned(),
        source_scope: source_scope.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: resolved_commit_sha.to_owned(),
        tree_hash: tree_hash.to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    }
}

async fn set_maintenance_marker(store: &SqliteGraphStore, marker: u64) {
    store
        .run(move |connection| {
            connection.execute(
                "INSERT INTO relay_sqlite_maintenance_diagnostics
                     (id, last_maintenance_at_ms, last_maintenance_error)
                 VALUES (1, ?1, NULL)
                 ON CONFLICT(id) DO UPDATE SET last_maintenance_at_ms = excluded.last_maintenance_at_ms",
                [marker],
            )?;
            Ok(())
        })
        .await
        .expect("maintenance marker should persist");
}

async fn maintenance_marker(store: &SqliteGraphStore) -> u64 {
    store
        .run(|connection| {
            connection
                .query_row(
                    "SELECT last_maintenance_at_ms
                     FROM relay_sqlite_maintenance_diagnostics WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(StorageError::from)
        })
        .await
        .expect("maintenance marker should load")
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow Unix epoch")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
