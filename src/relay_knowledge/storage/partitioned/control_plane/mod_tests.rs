use std::{path::PathBuf, sync::Arc};

use super::list_code_repositories;
use crate::{
    paths::RuntimePaths,
    storage::{SqliteGraphStore, partitioned::catalog::SqliteShardCatalog},
};

#[tokio::test]
async fn empty_control_plane_lists_no_repository_shards() {
    let control = Arc::new(SqliteGraphStore::open_in_memory().expect("control store should open"));
    let paths = runtime_paths();
    let store = super::super::PartitionedSqliteKnowledgeStore {
        control,
        catalog: Arc::new(SqliteShardCatalog::new(
            paths.data_dir.join("control.db"),
            paths,
        )),
    };

    assert!(
        list_code_repositories(&store)
            .await
            .expect("empty control plane should list")
            .is_empty()
    );
}

fn runtime_paths() -> RuntimePaths {
    RuntimePaths {
        config_dir: PathBuf::from("/tmp/relay-knowledge/config"),
        data_dir: PathBuf::from("/tmp/relay-knowledge/data"),
        state_dir: PathBuf::from("/tmp/relay-knowledge/state"),
        cache_dir: PathBuf::from("/tmp/relay-knowledge/cache"),
        log_dir: PathBuf::from("/tmp/relay-knowledge/log"),
        temp_dir: PathBuf::from("/tmp/relay-knowledge/temp"),
        runtime_dir: PathBuf::from("/tmp/relay-knowledge/run"),
        service_dir: PathBuf::from("/tmp/relay-knowledge/service"),
    }
}
