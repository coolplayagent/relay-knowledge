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
    let mut store = PartitionedSqliteKnowledgeStore::open(paths.database_file(), paths).unwrap();
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
    assert_eq!(result.last_maintenance_at_ms, Some(12));
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
