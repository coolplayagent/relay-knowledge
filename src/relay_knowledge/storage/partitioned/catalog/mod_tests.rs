use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};

use super::{
    catalog_active_repository_for_scope, initialize_catalog_schema, record_catalog_scope,
    shard_locator, stage_catalog_scope,
};
use crate::paths::RuntimePaths;

#[tokio::test]
async fn first_shard_open_validates_managed_paths_before_creating_sqlite() {
    let data_dir = unique_database_path("private-shard").with_extension("data");
    let mut paths = runtime_paths(data_dir.clone());
    paths.windows_data_sid = Some("S-1-5-21-1-2-3-1001".to_owned());
    let catalog = super::SqliteShardCatalog::new(
        paths.database_file(),
        paths,
        std::sync::Arc::new(crate::storage::SqliteGraphStore::open_in_memory().unwrap()),
    );
    let error = match catalog
        .staged_repository_store("private-repository".to_owned())
        .await
    {
        Ok(_) => panic!("invalid managed policy must not create a shard"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("account policy"));
    assert!(!data_dir.exists());
}

#[test]
fn shard_locator_is_relative_only_inside_the_runtime_data_directory() {
    let paths = runtime_paths(PathBuf::from("/var/lib/relay-knowledge"));
    let internal = paths.data_dir.join("repositories/repo.db");
    let external = PathBuf::from("/mnt/shards/repo.db");

    assert_eq!(shard_locator(&paths, &internal), "repositories/repo.db");
    assert_eq!(
        shard_locator(&paths, &external),
        external.display().to_string()
    );
}

#[test]
fn staging_an_active_scope_never_closes_its_read_gate() {
    let path = unique_database_path("active-stage-conflict");
    initialize_catalog_schema(&path).expect("catalog should initialize");
    record_catalog_scope(&path, "repo", "scope", "repo.db").expect("scope should activate");

    stage_catalog_scope(&path, "repo", "scope", "repo.db")
        .expect("idempotent staging should succeed");

    assert_eq!(
        catalog_active_repository_for_scope(&path, "scope")
            .expect("active route should query")
            .as_deref(),
        Some("repo")
    );
    let connection = Connection::open(&path).expect("catalog should reopen");
    let state = connection
        .query_row(
            "SELECT state FROM storage_repository_shard_scopes WHERE source_scope = ?1",
            params!["scope"],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .expect("scope state should query");
    assert_eq!(state.as_deref(), Some("active"));
    drop(connection);
    remove_database(&path);
}

fn runtime_paths(data_dir: PathBuf) -> RuntimePaths {
    RuntimePaths {
        windows_data_sid: None,
        config_dir: PathBuf::from("/etc/relay-knowledge"),
        data_dir,
        state_dir: PathBuf::from("/var/lib/relay-knowledge/state"),
        cache_dir: PathBuf::from("/var/cache/relay-knowledge"),
        log_dir: PathBuf::from("/var/log/relay-knowledge"),
        temp_dir: PathBuf::from("/tmp/relay-knowledge"),
        runtime_dir: PathBuf::from("/run/relay-knowledge"),
        service_dir: PathBuf::from("/etc/systemd/system"),
    }
}

fn unique_database_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "relay-knowledge-catalog-{label}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn remove_database(path: &std::path::Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        let _ = std::fs::remove_file(candidate);
    }
}
