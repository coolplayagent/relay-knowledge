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
        "relay-knowledge-partitioned-indexing-{name}-{}-{nanos}-{sequence}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    path
}
