use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    domain::CodeRepositoryRegistration,
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
    storage::{
        KnowledgeStoreFactory, PartitionedSqliteKnowledgeStore, RepositoryCatalogStore as _,
        StorageTopology,
    },
};

use super::*;

#[tokio::test]
async fn windows_storage_policy_is_checked_before_either_topology_opens_sqlite() {
    for topology in [
        StorageTopology::SingleSqlite,
        StorageTopology::PartitionedSqlite,
    ] {
        let mut paths = runtime_paths();
        paths.windows_data_sid = Some("S-1-5-21-1-2-3-1001".to_owned());
        let data = paths.data_dir.clone();
        let factory = SqliteKnowledgeStoreFactory::new(paths, topology);
        assert!(!data.exists(), "factory construction must stay lazy");
        let error = match factory.open().await {
            Ok(_) => panic!("mismatched automatic storage policy must fail"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("no longer matches its account policy")
        );
        assert!(
            factory
                .topology_snapshot()
                .await
                .unwrap_err()
                .to_string()
                .contains("no longer matches its account policy")
        );
        assert!(
            !data.exists(),
            "policy failure must precede SQLite directory creation"
        );
    }
}

#[tokio::test]
async fn read_only_topology_does_not_authorize_the_first_database_open() {
    let paths = runtime_paths();
    let data = paths.data_dir.clone();
    let mut factory = SqliteKnowledgeStoreFactory::new(paths, StorageTopology::SingleSqlite);
    assert!(factory.topology_snapshot().await.unwrap().shards.is_empty());
    assert!(!data.exists());
    // Make the next policy check fail, as a changed ACL would on Windows.
    factory.paths.windows_data_sid = Some("S-1-5-21-1-2-3-1001".to_owned());
    let error = match factory.open().await {
        Ok(_) => panic!("a diagnostic must not cache permission to open storage"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("account policy"));
    assert!(!data.exists());
}

#[tokio::test]
async fn topology_revalidates_paths_after_a_successful_store_open() {
    for topology in [
        StorageTopology::SingleSqlite,
        StorageTopology::PartitionedSqlite,
    ] {
        let paths = runtime_paths();
        let root = paths.data_dir.parent().unwrap().to_path_buf();
        let mut factory = SqliteKnowledgeStoreFactory::new(paths, topology);
        let store = factory.open().await.unwrap();
        factory.topology_snapshot().await.unwrap();
        // A valid open must not cache authorization for a later path-based
        // read. Simulate a policy failure without requiring Windows ACL APIs.
        factory.paths.windows_data_sid = Some("S-1-5-21-1-2-3-1001".to_owned());
        let error = factory.topology_snapshot().await.unwrap_err();
        assert!(error.to_string().contains("account policy"));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn single_sqlite_rejects_active_partitioned_catalog() {
    let paths = runtime_paths();
    let database_path = paths.database_file();
    let partitioned =
        PartitionedSqliteKnowledgeStore::open(&database_path, paths.clone()).expect("open");
    partitioned
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo-alpha",
                "alpha",
                "/tmp/alpha",
                Vec::new(),
                Vec::new(),
            )
            .expect("registration"),
        )
        .await
        .expect("partitioned registration activates catalog");

    let factory = SqliteKnowledgeStoreFactory::new(paths.clone(), StorageTopology::SingleSqlite);
    assert!(
        factory
            .validate_lifecycle_storage()
            .await
            .unwrap_err()
            .to_string()
            .contains("partitioned_sqlite")
    );
    SqliteKnowledgeStoreFactory::new(paths, StorageTopology::PartitionedSqlite)
        .validate_lifecycle_storage()
        .await
        .unwrap();
    let error = match factory.open().await {
        Ok(_) => panic!("single topology should reject active shard catalog"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("partitioned_sqlite"));
    assert!(error.to_string().contains("single_sqlite"));
}

fn runtime_paths() -> RuntimePaths {
    let root = unique_temp_dir("storage-provider");
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(
            "RELAY_KNOWLEDGE_HOME",
            root.to_str().expect("temp path should be UTF-8"),
        )],
    )
    .expect("environment should parse");

    RuntimePaths::resolve(&environment.platform, &environment.paths).expect("paths resolve")
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-{name}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}
