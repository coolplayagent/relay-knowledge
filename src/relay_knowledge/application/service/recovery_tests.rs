use std::sync::Arc;

use super::*;
use crate::{
    api::InterfaceKind,
    domain::{EvidenceRecord, GraphMutationBatch, IndexKind, SourceScope},
    env::{EnvironmentConfig, PlatformKind},
    storage::{GraphStore, KnowledgeStore, SqliteGraphStore},
};

#[tokio::test]
async fn health_metadata_preserves_stale_index_state() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    let evidence = EvidenceRecord::new(
        "ev-stale",
        SourceScope::parse("docs").expect("scope should parse"),
        "Direct storage writes leave indexes stale",
        vec!["Index".to_owned()],
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let health = service
        .health(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-health",
            "trace-health",
        ))
        .await
        .expect("health should load");

    assert!(!health.healthy);
    assert!(health.metadata.stale);
    assert_eq!(health.metadata.graph_version, 1);
}

#[tokio::test]
async fn startup_reconciler_refreshes_stale_index_cursors() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    let evidence = EvidenceRecord::new(
        "ev-startup-recovery",
        SourceScope::parse("docs").expect("scope should parse"),
        "Startup recovery refreshes stale derived indexes",
        vec!["Recovery".to_owned()],
    )
    .expect("evidence should validate");
    store
        .commit_mutation_batch(
            GraphMutationBatch::new(vec![evidence]).expect("batch should validate"),
        )
        .await
        .expect("commit should succeed");

    let report = service
        .reconcile_startup_indexes(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-reconcile",
            "trace-reconcile",
        ))
        .await
        .expect("reconciler should refresh stale indexes");
    let health = service
        .health(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-health",
            "trace-health",
        ))
        .await
        .expect("health should load");

    assert_eq!(report.stale_index_kinds, IndexKind::ALL);
    assert_eq!(report.refreshed_index_kinds, IndexKind::ALL);
    assert!(report.index_lag_max > 0);
    assert_eq!(report.heartbeat_state, "ready");
    assert!(health.healthy);
    assert!(!health.metadata.stale);
}

async fn service_with_store(store: Arc<dyn KnowledgeStore>) -> RelayKnowledgeService {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
        ],
    )
    .expect("environment should parse");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
}
