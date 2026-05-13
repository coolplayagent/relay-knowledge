use std::sync::Arc;

use super::*;
use crate::{
    api::InterfaceKind,
    domain::{EvidenceRecord, GraphMutationBatch, IndexKind, IndexState, SourceScope},
    env::{EnvironmentConfig, PlatformKind},
    storage::{GraphStore, KnowledgeStore, SqliteGraphStore},
};

#[tokio::test]
async fn health_queues_scoped_backlogs_larger_than_initial_budget() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    store
        .commit_mutation_batch(
            GraphMutationBatch::new(scoped_evidence("health-large", 43)).expect("batch"),
        )
        .await
        .expect("commit should succeed");

    let health = service
        .health(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-health-large",
            "trace-health",
        ))
        .await
        .expect("health should degrade with queued work instead of failing");

    assert!(!health.healthy);
    assert_eq!(health.index_refresh.queue_depth, 129);
    assert_eq!(health.index_cursors.len(), 129);
}

#[tokio::test]
async fn refresh_indexes_drains_scoped_backlogs_larger_than_single_page() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service = service_with_store(store.clone()).await;
    store
        .commit_mutation_batch(
            GraphMutationBatch::new(scoped_evidence("refresh-large", 90)).expect("batch"),
        )
        .await
        .expect("commit should succeed");

    let refreshed = service
        .refresh_indexes(
            IndexRefreshRequest { kinds: Vec::new() },
            RequestContext::with_ids(InterfaceKind::Cli, "req-refresh-large", "trace-refresh"),
        )
        .await
        .expect("refresh should drain all queued scoped tasks");

    assert!(!refreshed.metadata.stale);
    assert_eq!(refreshed.diagnostics.queue_depth, 0);
    assert_eq!(refreshed.index_cursors.len(), 270);
    assert!(
        refreshed
            .indexes
            .iter()
            .all(|status| status.state == IndexState::Fresh)
    );
}

#[tokio::test]
async fn refresh_indexes_excludes_disabled_read_models_from_outcome() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let environment = disabled_read_model_environment();
    let service = service_with_environment_and_store(&environment, store.clone()).await;
    store
        .commit_mutation_batch(
            GraphMutationBatch::new(scoped_evidence("disabled-refresh", 1)).expect("batch"),
        )
        .await
        .expect("commit should succeed");

    let refreshed = service
        .refresh_indexes(
            IndexRefreshRequest { kinds: Vec::new() },
            RequestContext::with_ids(InterfaceKind::Cli, "req-refresh-disabled", "trace-refresh"),
        )
        .await
        .expect("enabled indexes should refresh");

    assert!(!refreshed.metadata.stale);
    assert_eq!(
        refreshed
            .indexes
            .iter()
            .map(|status| status.kind)
            .collect::<Vec<_>>(),
        vec![IndexKind::Bm25]
    );
    assert!(
        refreshed
            .index_cursors
            .iter()
            .all(|cursor| cursor.kind == IndexKind::Bm25)
    );
    assert_eq!(
        refreshed
            .diagnostics
            .index_lag_by_kind
            .iter()
            .map(|lag| lag.kind)
            .collect::<Vec<_>>(),
        vec![IndexKind::Bm25]
    );
    assert_eq!(refreshed.diagnostics.stale_index_count, 0);
    assert!(
        refreshed
            .diagnostics
            .stale_reasons
            .iter()
            .all(|reason| reason.kind == IndexKind::Bm25)
    );
}

#[tokio::test]
async fn health_and_service_status_exclude_disabled_read_model_diagnostics() {
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let environment = disabled_read_model_environment();
    let service = service_with_environment_and_store(&environment, store.clone()).await;
    store
        .commit_mutation_batch(
            GraphMutationBatch::new(scoped_evidence("disabled-health", 1)).expect("batch"),
        )
        .await
        .expect("commit should succeed");
    service
        .refresh_indexes(
            IndexRefreshRequest { kinds: Vec::new() },
            RequestContext::with_ids(InterfaceKind::Cli, "req-health-refresh", "trace-refresh"),
        )
        .await
        .expect("enabled indexes should refresh");

    let health = service
        .health(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-disabled-health",
            "trace-health",
        ))
        .await
        .expect("health should load");
    let status = service
        .service_status(RequestContext::with_ids(
            InterfaceKind::Cli,
            "req-disabled-service-status",
            "trace-status",
        ))
        .await
        .expect("service status should load");

    assert!(health.healthy);
    assert!(!health.metadata.stale);
    assert_eq!(
        health
            .indexes
            .iter()
            .map(|status| status.kind)
            .collect::<Vec<_>>(),
        vec![IndexKind::Bm25]
    );
    assert!(
        health
            .index_cursors
            .iter()
            .all(|cursor| cursor.kind == IndexKind::Bm25)
    );
    assert_eq!(health.index_refresh.stale_index_count, 0);
    assert!(
        health
            .index_refresh
            .stale_reasons
            .iter()
            .all(|reason| reason.kind == IndexKind::Bm25)
    );
    assert_eq!(status.index_refresh.stale_index_count, 0);
    assert!(
        status
            .index_refresh
            .stale_reasons
            .iter()
            .all(|reason| reason.kind == IndexKind::Bm25)
    );
}

fn scoped_evidence(prefix: &str, count: usize) -> Vec<EvidenceRecord> {
    (0..count)
        .map(|index| {
            EvidenceRecord::new(
                format!("ev-{prefix}-{index}"),
                SourceScope::parse(format!("scope{index}")).expect("scope should parse"),
                format!("Scoped refresh evidence {prefix} {index}"),
                vec!["Index".to_owned()],
            )
            .expect("evidence should validate")
        })
        .collect()
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

    service_with_environment_and_store(&environment, store).await
}

async fn service_with_environment_and_store(
    environment: &EnvironmentConfig,
    store: Arc<dyn KnowledgeStore>,
) -> RelayKnowledgeService {
    let runtime = RuntimeConfiguration::from_environment(environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
}

fn disabled_read_model_environment() -> EnvironmentConfig {
    EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            ("RELAY_KNOWLEDGE_HOME", "/srv/relay"),
            ("RELAY_KNOWLEDGE_SEMANTIC_BACKEND", "disabled"),
            ("RELAY_KNOWLEDGE_VECTOR_BACKEND", "disabled"),
        ],
    )
    .expect("environment should parse")
}
