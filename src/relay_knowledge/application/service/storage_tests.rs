use std::sync::Arc;

use crate::{
    application::RelayKnowledgeService,
    env::{EnvironmentConfig, PlatformKind},
};

#[tokio::test]
async fn concurrent_storage_initialization_returns_canonical_store() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-storage-race-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("TMPDIR", "/tmp"),
            (
                "RELAY_KNOWLEDGE_HOME",
                root.to_str().expect("temp path is UTF-8"),
            ),
        ],
    )
    .expect("environment should parse");
    let service = Arc::new(
        RelayKnowledgeService::from_environment(&environment)
            .await
            .expect("service should compose"),
    );
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let service = Arc::clone(&service);
        tasks.push(tokio::spawn(async move {
            service
                .storage
                .get()
                .await
                .expect("store should initialize")
        }));
    }

    let mut stores = Vec::new();
    for task in tasks {
        stores.push(task.await.expect("task should join"));
    }

    let first = stores.first().expect("stores should exist");
    assert!(stores.iter().all(|store| Arc::ptr_eq(first, store)));
}
