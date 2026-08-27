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
        CodeRepositoryStore, KnowledgeStoreFactory, PartitionedSqliteKnowledgeStore,
        StorageTopology,
    },
};

use super::*;

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

    let error = match SqliteKnowledgeStoreFactory::new(paths, StorageTopology::SingleSqlite)
        .open()
        .await
    {
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
