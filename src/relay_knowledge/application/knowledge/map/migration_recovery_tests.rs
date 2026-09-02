use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::fs;

use super::*;
use crate::{
    api::{InterfaceKind, RequestContext},
    project::{
        AGENT_CONTRACT_DIR_NAME, KNOWLEDGE_MAP_TOPICS_DIR_NAME, LEGACY_AGENT_CONTRACT_DIR_NAME,
    },
};

use super::super::{
    KnowledgeMapTopicShard, content_digest, parse_manifest, serialize_yaml, stable_id,
};

#[tokio::test]
async fn migration_refreshes_the_rollback_backup_from_the_active_v2_root() {
    let (root, service, context) = migrated_v2_fixture("refresh-v2-backup").await;

    service
        .rollback_v3(&context)
        .await
        .expect("first rollback should restore the v2 root");
    let mut manifest = parse_manifest(
        &fs::read_to_string(service.legacy_map_path())
            .await
            .expect("restored v2 root should read"),
    )
    .expect("restored v2 root should parse");
    let topic_ref = manifest
        .topics
        .iter_mut()
        .find(|topic| topic.id == "software-model")
        .expect("v2 map should retain the software-model topic");
    let mut shard: KnowledgeMapTopicShard = serde_norway::from_str(
        &fs::read_to_string(
            root.join(LEGACY_AGENT_CONTRACT_DIR_NAME)
                .join(&topic_ref.r#ref),
        )
        .await
        .expect("v2 software shard should read"),
    )
    .expect("v2 software shard should parse");
    let source = shard
        .sources
        .iter_mut()
        .find(|source| source.id == "repository-software-model")
        .expect("v2 software source should exist");
    source.description = Some("Refreshed by a v2 writer after rollback.".to_owned());
    source.version = source.version.saturating_add(1);
    let shard_yaml = serialize_yaml(&shard).expect("updated v2 shard should serialize");
    topic_ref.digest = content_digest(shard_yaml.as_bytes());
    topic_ref.r#ref = format!(
        "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{}.yaml",
        stable_id(&topic_ref.id),
        topic_ref.digest
    );
    fs::write(
        root.join(LEGACY_AGENT_CONTRACT_DIR_NAME)
            .join(&topic_ref.r#ref),
        shard_yaml,
    )
    .await
    .expect("updated v2 shard should write");
    let active_v2_root = serialize_yaml(&manifest).expect("updated v2 root should serialize");
    fs::write(service.legacy_map_path(), &active_v2_root)
        .await
        .expect("v2 writer should update the active root");

    service
        .migrate_to_v3(&context)
        .await
        .expect("second migration should snapshot the active v2 root");
    assert_eq!(
        fs::read_to_string(service.legacy_backup_path())
            .await
            .expect("refreshed rollback backup should read"),
        active_v2_root
    );

    service
        .rollback_v3(&context)
        .await
        .expect("second rollback should restore the refreshed v2 root");
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("restored refreshed root should read"),
        active_v2_root
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rollback_recovery_promotes_a_retained_fallback_only_root() {
    let (root, service, _context) = migrated_v2_fixture("fallback-only-rollback").await;
    let current_before = fs::read(service.map_path())
        .await
        .expect("v3 root should read");
    let legacy_before = fs::read(service.legacy_map_path())
        .await
        .expect("legacy redirect should read");

    fs::remove_file(service.backup_path())
        .await
        .expect("ordinary fallback should clear before simulating publication residue");
    fs::rename(service.map_path(), service.backup_path())
        .await
        .expect("manifest publication should leave only the ordinary fallback");
    fs::copy(
        service.legacy_backup_path(),
        service.legacy_rollback_prepared_path(),
    )
    .await
    .expect("rollback root should stage");
    fs::rename(service.backup_path(), service.retained_v3_backup_path())
        .await
        .expect("rollback should retain the fallback root");
    fs::rename(
        service.legacy_map_path(),
        service.legacy_rollback_previous_path(),
    )
    .await
    .expect("rollback should retain the legacy redirect");

    let _legacy_lock = service
        .acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT)
        .await
        .expect("legacy lock should acquire");
    let _current_lock = service
        .acquire_write_lock(WRITE_LOCK_TIMEOUT)
        .await
        .expect("current lock should acquire");
    assert!(
        !service
            .recover_legacy_rollback_transition()
            .await
            .expect("restart recovery should restore the interrupted rollback")
    );

    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("fallback root should become visible again"),
        current_before
    );
    assert_eq!(
        fs::read(service.legacy_map_path())
            .await
            .expect("legacy redirect should recover"),
        legacy_before
    );
    assert!(!fs::try_exists(service.retained_v3_path()).await.unwrap());
    assert!(
        !fs::try_exists(service.retained_v3_backup_path())
            .await
            .unwrap()
    );
    assert!(
        !fs::try_exists(service.legacy_rollback_prepared_path())
            .await
            .unwrap()
    );
    assert!(
        !fs::try_exists(service.legacy_rollback_previous_path())
            .await
            .unwrap()
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rollback_transition_keeps_the_retained_v3_contract_readable() {
    let (root, service, context) = migrated_v2_fixture("readable-rollback-transition").await;
    let current_before = fs::read_to_string(service.map_path())
        .await
        .expect("v3 root should read");

    fs::copy(
        service.legacy_backup_path(),
        service.legacy_rollback_prepared_path(),
    )
    .await
    .expect("legacy rollback root should stage");
    fs::rename(service.map_path(), service.retained_v3_path())
        .await
        .expect("current v3 root should move to retained recovery state");
    fs::rename(service.backup_path(), service.retained_v3_backup_path())
        .await
        .expect("fallback v3 root should move to retained recovery state");

    assert_eq!(
        service
            .read_root_content()
            .await
            .expect("retained v3 root should remain readable during rollback"),
        current_before
    );
    assert_eq!(
        service
            .read_contract_dir_name()
            .await
            .expect("retained root must keep the v3 contract directory"),
        AGENT_CONTRACT_DIR_NAME
    );
    assert!(
        service
            .route(&context, "software-model".to_owned())
            .await
            .expect("route reads should use the retained v3 root")
            .route
            .is_some()
    );

    let _legacy_lock = service
        .acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT)
        .await
        .expect("legacy lock should acquire");
    let _current_lock = service
        .acquire_write_lock(WRITE_LOCK_TIMEOUT)
        .await
        .expect("current lock should acquire");
    assert!(
        !service
            .recover_legacy_rollback_transition()
            .await
            .expect("recovery should restore the v3 root before legacy publication")
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn committed_rollback_recovery_keeps_post_commit_legacy_edits_when_cleanup_is_retried() {
    let (root, service, _context) = migrated_v2_fixture("rollback-cleanup-residue").await;

    fs::copy(
        service.legacy_backup_path(),
        service.legacy_rollback_prepared_path(),
    )
    .await
    .expect("legacy rollback root should stage");
    fs::rename(service.map_path(), service.retained_v3_path())
        .await
        .expect("current v3 root should retain");
    fs::rename(service.backup_path(), service.retained_v3_backup_path())
        .await
        .expect("fallback v3 root should retain");
    fs::rename(
        service.legacy_map_path(),
        service.legacy_rollback_previous_path(),
    )
    .await
    .expect("legacy redirect should retain");
    fs::rename(
        service.legacy_rollback_prepared_path(),
        service.legacy_map_path(),
    )
    .await
    .expect("legacy rollback publication should commit");

    let mut legacy = parse_manifest(
        &fs::read_to_string(service.legacy_map_path())
            .await
            .expect("committed legacy root should read"),
    )
    .expect("committed legacy root should parse");
    legacy.updated_at = "unix:2".to_owned();
    let edited_legacy = serialize_yaml(&legacy).expect("edited legacy root should serialize");
    fs::write(service.legacy_map_path(), &edited_legacy)
        .await
        .expect("v2 writer should update the committed legacy root");

    let _legacy_lock = service
        .acquire_legacy_write_lock(WRITE_LOCK_TIMEOUT)
        .await
        .expect("legacy lock should acquire");
    let _current_lock = service
        .acquire_write_lock(WRITE_LOCK_TIMEOUT)
        .await
        .expect("current lock should acquire");
    assert!(
        service
            .recover_legacy_rollback_transition()
            .await
            .expect("recovery should only clean committed rollback residue")
    );

    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("edited legacy root should remain active"),
        edited_legacy
    );
    assert!(
        !fs::try_exists(service.legacy_rollback_previous_path())
            .await
            .unwrap()
    );
    assert!(fs::try_exists(service.retained_v3_path()).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn migration_rejects_an_oversized_unreferenced_legacy_artifact() {
    let (root, service, context) = legacy_v2_fixture("bounded-legacy-artifact").await;
    let oversized = root
        .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
        .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME)
        .join("unreferenced-large.yaml");
    fs::write(
        &oversized,
        vec![b'x'; (MAX_LEGACY_MIGRATION_ARTIFACT_FILE_BYTES + 1) as usize],
    )
    .await
    .expect("oversized unreferenced artifact should write");

    let error = service
        .migrate_to_v3(&context)
        .await
        .expect_err("oversized legacy artifacts must be rejected before loading their contents");

    assert!(error.to_string().contains("legacy migration artifact"));
    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

async fn migrated_v2_fixture(label: &str) -> (PathBuf, KnowledgeMapService, RequestContext) {
    let (root, service, context) = legacy_v2_fixture(label).await;
    service
        .migrate_to_v3(&context)
        .await
        .expect("v2 fixture should migrate");
    (root, service, context)
}

async fn legacy_v2_fixture(label: &str) -> (PathBuf, KnowledgeMapService, RequestContext) {
    let root = temp_root(label);
    fs::create_dir_all(&root)
        .await
        .expect("repository root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    service
        .init(&context)
        .await
        .expect("v3 fixture should create");
    fs::rename(
        root.join(AGENT_CONTRACT_DIR_NAME),
        root.join(LEGACY_AGENT_CONTRACT_DIR_NAME),
    )
    .await
    .expect("fixture contract should move to the legacy root");
    let legacy = service.legacy_map_path();
    let v2 = fs::read_to_string(&legacy)
        .await
        .expect("fixture root should read")
        .replacen("schema_version: 3", "schema_version: 2", 1);
    fs::write(&legacy, v2)
        .await
        .expect("v2 fixture root should write");
    (root, service, context)
}

fn temp_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("relay-knowledge-{name}-{nonce}"))
}
