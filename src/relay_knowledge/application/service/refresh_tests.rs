use std::sync::Arc;

use super::*;
use crate::{
    api::InterfaceKind,
    domain::{EvidenceRecord, GraphMutationBatch, IndexState, SourceScope},
    env::PlatformKind,
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
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");

    RelayKnowledgeService::with_store(runtime, store)
}
