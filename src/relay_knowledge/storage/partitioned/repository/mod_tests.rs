use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{remove, status, upsert};
use crate::{
    domain::CodeRepositoryRegistration,
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn repository_lifecycle_preserves_alias_and_removes_catalog_routing() {
    let store = partitioned_store("repository-lifecycle");
    let registration = CodeRepositoryRegistration::new(
        "repo-id",
        "fixture-alias",
        "/tmp/fixture",
        Vec::new(),
        Vec::new(),
    )
    .expect("registration should validate");

    let registered = upsert(&store, registration)
        .await
        .expect("repository should register in control and shard stores");
    assert_eq!(registered.alias, "fixture-alias");
    assert_eq!(
        status(&store, "fixture-alias".to_owned())
            .await
            .expect("repository status should load")
            .expect("registered repository should exist")
            .alias,
        "fixture-alias"
    );

    let removed = remove(&store, "fixture-alias".to_owned(), 10)
        .await
        .expect("repository removal should succeed");
    assert!(removed.is_some());
    assert!(
        status(&store, "fixture-alias".to_owned())
            .await
            .expect("removed repository lookup should succeed")
            .is_none()
    );
}

fn partitioned_store(name: &str) -> super::super::PartitionedSqliteKnowledgeStore {
    let root = unique_temp_dir(name);
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(
            "RELAY_KNOWLEDGE_HOME",
            root.to_str().expect("temp root should be UTF-8"),
        )],
    )
    .expect("environment should parse");
    let paths =
        RuntimePaths::resolve(&environment.platform, &environment.paths).expect("paths resolve");
    super::super::PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths)
        .expect("store should open")
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let sequence = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-partitioned-{name}-{}-{nanos}-{sequence}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}
