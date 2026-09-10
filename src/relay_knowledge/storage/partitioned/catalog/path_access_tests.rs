use super::*;

#[test]
fn synchronous_partitioned_open_validates_before_creating_the_control_database() {
    let root = std::env::temp_dir().join("relay-control-admission");
    let environment = crate::env::EnvironmentConfig::from_pairs(
        crate::env::PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap())],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
    let invalid =
        Path::new("D:/relay-knowledge/users/S-1-invalid-initial-open/data/relay-knowledge.sqlite");
    let error = crate::storage::PartitionedSqliteKnowledgeStore::open(invalid, paths)
        .err()
        .unwrap();
    assert!(error.to_string().contains("invalid account SID"));
    assert!(
        !invalid.exists(),
        "admission must precede SQLite creation and migration"
    );
}

#[test]
fn fresh_catalog_reads_and_writes_reject_invalid_managed_paths_before_sqlite() {
    let path = Path::new("D:/relay-knowledge/users/S-1-invalid/data/relay-knowledge.sqlite");
    assert!(
        open_catalog_connection(path)
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    assert!(
        open_catalog_readonly_connection(path)
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    assert!(
        upsert_catalog_repository(path, "repo", "shard.sqlite")
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    assert!(
        catalog_repository_for_scope(path, "scope")
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    assert!(!path.exists());
}

#[tokio::test]
async fn inspection_catalog_reads_retain_the_control_handle_and_bound_shard_count() {
    use crate::storage::GraphStore;
    let root = std::env::temp_dir().join(format!(
        "relay-inspection-catalog-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let environment = crate::env::EnvironmentConfig::from_pairs(
        crate::env::PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap())],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
    let mut store =
        crate::storage::PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths)
            .unwrap();
    store
        .control
        .run(|connection| {
            connection.execute(
                "INSERT INTO storage_repository_shards VALUES ('first', 'unused', 'active', 0, 0)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    Arc::get_mut(&mut store.catalog).unwrap().control_path = PathBuf::from(
        "D:/relay-knowledge/users/S-1-invalid-inspection/data/relay-knowledge.sqlite",
    );
    let inspection = store.inspect_graph().await.unwrap();
    assert!(
        inspection
            .sqlite
            .last_maintenance_error
            .unwrap()
            .contains("shard first")
    );
    store.control.run(|connection| {
        let limit = crate::storage::sqlite::MAX_SQLITE_DIAGNOSTIC_SHARDS;
        connection.execute("WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM ids WHERE n < ?1) INSERT INTO storage_repository_shards SELECT 'repo-' || n, 'unused', 'active', 0, 0 FROM ids", [limit - 1])?;
        Ok(())
    }).await.unwrap();
    assert_eq!(
        store
            .catalog
            .diagnostic_repository_ids()
            .await
            .unwrap()
            .len(),
        crate::storage::sqlite::MAX_SQLITE_DIAGNOSTIC_SHARDS
    );
    store.control.run(|connection| {
        connection.execute("INSERT INTO storage_repository_shards VALUES ('overflow', 'unused', 'active', 0, 0)", [])?;
        Ok(())
    }).await.unwrap();
    assert!(
        store
            .catalog
            .diagnostic_repository_ids()
            .await
            .unwrap_err()
            .to_string()
            .contains("exceeds")
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
