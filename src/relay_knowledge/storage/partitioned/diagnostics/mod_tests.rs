use super::SqliteDiagnosticsAggregate;
use crate::storage::{SqliteStorageDiagnostics, StorageError};

#[tokio::test]
async fn warm_health_reuses_validated_shards_without_a_payload_security_scan() {
    use crate::{
        domain::CodeRepositoryRegistration,
        env::{EnvironmentConfig, PlatformKind},
        paths::RuntimePaths,
        storage::{GraphStore, PartitionedSqliteKnowledgeStore, RepositoryCatalogStore},
    };
    let root = std::env::temp_dir().join(format!(
        "relay-health-handles-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let env = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap())],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&env.platform, &env.paths).unwrap();
    let mut store =
        PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths.clone()).unwrap();
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "health-handles",
                "health",
                "/tmp/health-handles",
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let baseline = store.health_snapshot(0).await.unwrap();
    // Any new security process would fail this policy on every platform. Warm
    // health reads retain validated handles instead; full inspection still checks.
    std::sync::Arc::get_mut(&mut store.catalog)
        .unwrap()
        .paths
        .windows_data_sid = Some("S-1-5-21-1-2-3-1001".to_owned());
    let health = store.health_snapshot(0).await.unwrap();
    assert_eq!(
        health.repository_code_totals,
        baseline.repository_code_totals
    );
    assert_eq!(health.graph.sqlite, baseline.graph.sqlite);
    assert!(
        store
            .inspect_graph()
            .await
            .unwrap_err()
            .to_string()
            .contains("account policy")
    );
    drop(store);
    let mut cold = PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths).unwrap();
    std::sync::Arc::get_mut(&mut cold.catalog)
        .unwrap()
        .paths
        .windows_data_sid = Some("S-1-5-21-1-2-3-1001".to_owned());
    for _ in 0..3 {
        let error = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            cold.health_snapshot(0),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(
            matches!(error, StorageError::Busy(message) if message.starts_with("storage_cold:"))
        );
    }
    drop(cold);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn aggregate_reports_mixed_journals_saturating_wal_and_labeled_errors() {
    let mut aggregate = SqliteDiagnosticsAggregate::new();
    aggregate.push("control", diagnostics("wal", Some(u64::MAX), Some(10)));
    aggregate.push("shard repo", diagnostics("delete", Some(8), Some(12)));
    aggregate.push_error(
        "shard missing",
        StorageError::InvalidInput("repository shard is missing".to_owned()),
    );

    let result = aggregate.finish();
    assert_eq!(result.journal_mode, "mixed");
    assert_eq!(result.wal_size_bytes, None);
    assert_eq!(result.last_maintenance_at_ms, None);
    assert!(
        result
            .last_maintenance_error
            .expect("missing shard should be reported")
            .contains("shard missing")
    );
}

fn diagnostics(
    journal_mode: &str,
    wal_size_bytes: Option<u64>,
    last_maintenance_at_ms: Option<u64>,
) -> SqliteStorageDiagnostics {
    SqliteStorageDiagnostics {
        journal_mode: journal_mode.to_owned(),
        wal_size_bytes,
        last_maintenance_at_ms,
        last_maintenance_error: None,
    }
}

#[test]
fn maintenance_summary_preserves_oldest_and_missing_history_in_any_order() {
    for times in [[Some(10), Some(12)], [Some(12), Some(10)]] {
        let mut aggregate = SqliteDiagnosticsAggregate::new();
        for time in times {
            aggregate.push("database", diagnostics("wal", Some(0), time));
        }
        assert_eq!(aggregate.finish().last_maintenance_at_ms, Some(10));
    }
    for times in [[None, Some(12)], [Some(12), None]] {
        let mut aggregate = SqliteDiagnosticsAggregate::new();
        for time in times {
            aggregate.push("database", diagnostics("wal", Some(0), time));
        }
        assert_eq!(aggregate.finish().last_maintenance_at_ms, None);
    }
    assert_eq!(
        SqliteDiagnosticsAggregate::new()
            .finish()
            .last_maintenance_at_ms,
        None
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partitioned_health_never_queues_catalog_or_totals_reads_behind_busy_pools() {
    use crate::{
        domain::CodeRepositoryRegistration,
        env::{EnvironmentConfig, PlatformKind},
        paths::RuntimePaths,
        storage::{GraphStore, PartitionedSqliteKnowledgeStore, RepositoryCatalogStore},
    };
    use std::{
        sync::Arc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!(
        "relay-health-pool-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let env = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.to_str().unwrap())],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&env.platform, &env.paths).unwrap();
    let store = PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths).unwrap();
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "health-pool",
                "pool",
                "/tmp/health-pool",
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let shard = store
        .catalog
        .existing_repository_store("health-pool".into())
        .await
        .unwrap()
        .unwrap();
    for database in [Arc::clone(&store.control), shard] {
        let (started, mut ready) = tokio::sync::mpsc::channel(4);
        let mut releases = Vec::new();
        let mut readers = Vec::new();
        for _ in 0..4 {
            let database = Arc::clone(&database);
            let started = started.clone();
            let (release, held) = std::sync::mpsc::channel();
            releases.push(release);
            readers.push(tokio::spawn(async move {
                database
                    .run_read(move |_| {
                        started.blocking_send(()).unwrap();
                        let _ = held.recv_timeout(Duration::from_secs(5));
                        Ok(())
                    })
                    .await
                    .unwrap();
            }));
        }
        for _ in 0..4 {
            tokio::time::timeout(Duration::from_secs(5), ready.recv())
                .await
                .unwrap()
                .unwrap();
        }
        let health =
            tokio::time::timeout(Duration::from_millis(200), store.health_snapshot(0)).await;
        for release in releases {
            let _ = release.send(());
        }
        for reader in readers {
            reader.await.unwrap();
        }
        assert!(
            matches!(health, Ok(Err(StorageError::Busy(_)))),
            "health must reject immediately instead of detaching queued reads: {health:?}"
        );
        assert!(store.health_snapshot(0).await.is_ok());
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
