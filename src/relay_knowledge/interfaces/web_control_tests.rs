use super::*;
use axum::{body::to_bytes, http::Request};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

use crate::{
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{EvidenceRecord, GraphMutationBatch, SourceScope},
    env::{EnvironmentConfig, PlatformKind},
    storage::{GraphStore, KnowledgeStore, SqliteGraphStore},
};

#[tokio::test]
async fn control_service_status_does_not_queue_index_refresh_work() {
    let environment = control_test_environment("control-readonly-status");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let store = Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let service =
        RelayKnowledgeService::with_store(runtime, store.clone() as Arc<dyn KnowledgeStore>);
    store
        .commit_mutation_batch(
            GraphMutationBatch::new(vec![
                EvidenceRecord::new(
                    "ev-control-readonly",
                    SourceScope::parse("docs").expect("scope should parse"),
                    "Control status should observe stale indexes without queuing work",
                    vec!["Control".to_owned()],
                )
                .expect("evidence should validate"),
            ])
            .expect("batch should validate"),
        )
        .await
        .expect("mutation should commit");
    let router = router(service, crate::net::http::DEFAULT_MAX_BODY_BYTES);

    let read_only = get_json(router.clone(), "/api/v1/control/service/status").await;
    assert_eq!(read_only["index_refresh"]["queue_depth"], 0);

    let legacy = get_json(router, "/api/service/status").await;
    assert!(
        legacy["index_refresh"]["queue_depth"]
            .as_u64()
            .expect("queue depth should serialize")
            > 0
    );
}

#[tokio::test]
async fn cold_control_status_and_topology_do_not_open_partitioned_storage() {
    let environment = partitioned_control_test_environment("cold-control-status");
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .expect("runtime should compose");
    let database_path = runtime.paths.database_file();
    let router = router(
        RelayKnowledgeService::new(runtime),
        crate::net::http::DEFAULT_MAX_BODY_BYTES,
    );

    let status = get_json(router.clone(), "/api/v1/control/status").await;
    let topology = get_json(router, "/api/v1/control/storage/topology").await;

    assert_eq!(status["metadata"]["graph_version"], 0);
    assert_eq!(topology["metadata"]["graph_version"], 0);
    assert_eq!(topology["storage"]["topology"], "partitioned_sqlite");
    assert!(!database_path.exists());
}

fn control_test_environment(label: &str) -> EnvironmentConfig {
    let home = unique_temp_dir(label);
    EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/tmp"),
            (
                "RELAY_KNOWLEDGE_HOME",
                home.as_path().to_str().expect("home path should be utf8"),
            ),
        ],
    )
    .expect("environment should parse")
}

fn partitioned_control_test_environment(label: &str) -> EnvironmentConfig {
    let home = unique_temp_dir(label);
    EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/tmp"),
            (
                "RELAY_KNOWLEDGE_HOME",
                home.as_path().to_str().expect("home path should be utf8"),
            ),
            ("RELAY_KNOWLEDGE_STORAGE_TOPOLOGY", "partitioned_sqlite"),
        ],
    )
    .expect("environment should parse")
}

async fn get_json(router: Router, uri: &str) -> Value {
    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    serde_json::from_slice(&bytes).expect("response should be json")
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("relay-knowledge-web-control-{label}-{now}"))
}
