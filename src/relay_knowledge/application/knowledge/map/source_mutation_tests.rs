use std::time::{SystemTime, UNIX_EPOCH};

use tokio::{
    fs,
    time::{Duration, timeout},
};

use super::*;

#[tokio::test]
async fn mutations_hold_the_legacy_lock_when_no_legacy_state_exists() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-map-legacy-lock-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should work")
            .as_nanos()
    ));
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service
        .init(&context)
        .await
        .expect("current map should initialize");
    assert!(
        !service
            .legacy_recovery_state_exists()
            .await
            .expect("legacy state should be absent")
    );

    let legacy_lock = service
        .acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT)
        .await
        .expect("legacy lock should acquire");
    assert!(
        timeout(
            Duration::from_millis(100),
            service.acquire_legacy_aware_mutation_locks(),
        )
        .await
        .is_err(),
        "current mutations must wait for a legacy writer even before legacy artifacts exist"
    );
    drop(legacy_lock);

    let locks = service
        .acquire_legacy_aware_mutation_locks()
        .await
        .expect("locks should acquire after the legacy writer releases them");
    assert!(!locks.legacy_recovery_state);
    drop(locks);
    let _ = fs::remove_dir_all(root).await;
}
