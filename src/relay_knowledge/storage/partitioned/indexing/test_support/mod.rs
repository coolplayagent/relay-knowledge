use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn partitioned_store(name: &str) -> super::super::PartitionedSqliteKnowledgeStore {
    partitioned_store_with_paths(name).0
}

pub(super) fn partitioned_store_with_paths(
    name: &str,
) -> (
    super::super::PartitionedSqliteKnowledgeStore,
    PathBuf,
    RuntimePaths,
) {
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
    let control_path = paths.database_file();
    let store =
        super::super::PartitionedSqliteKnowledgeStore::open(control_path.clone(), paths.clone())
            .expect("store should open");
    (store, control_path, paths)
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let sequence = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-partitioned-indexing-{name}-{}-{nanos}-{sequence}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}
