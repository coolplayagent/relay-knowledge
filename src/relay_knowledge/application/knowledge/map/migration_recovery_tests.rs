use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::fs;

use super::*;
use crate::{
    api::{InterfaceKind, RequestContext},
    domain::{BusinessGlossary, DirectoryLoadHint, DirectoryUpdateRule, RepositoryMapDirectory},
    project::{
        AGENT_CONTRACT_DIR_NAME, BUSINESS_GLOSSARY_RELATIVE_PATH, KNOWLEDGE_MAP_TOPICS_DIR_NAME,
        LEGACY_AGENT_CONTRACT_DIR_NAME, LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH,
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
async fn directory_mutation_republishes_a_clean_legacy_rollback() {
    let (root, service, context) = migrated_v2_fixture("directory-mutation-clean-rollback").await;

    service
        .rollback_v3(&context)
        .await
        .expect("rollback should restore the v2 root");
    let directory = root.join("knowledge/integrations");
    fs::create_dir_all(&directory)
        .await
        .expect("governed directory should create");
    fs::write(directory.join("README.md"), "# Integrations\n")
        .await
        .expect("governed directory readme should write");

    service
        .add_directory(
            &context,
            RepositoryMapDirectory {
                directory: "integrations".to_owned(),
                purpose: "Integration knowledge.".to_owned(),
                content_scope: vec!["knowledge/integrations/**".to_owned()],
                key_files: vec!["knowledge/integrations/README.md".to_owned()],
                load_hint: DirectoryLoadHint::OnDemand,
                relations: Vec::new(),
                update_rule: DirectoryUpdateRule::Reviewed,
            },
        )
        .await
        .expect("directory mutation should migrate and republish the legacy contract");

    assert!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("legacy root should read")
            .contains("artifact_kind: redirect")
    );
    assert!(
        service
            .show(&context, None)
            .await
            .expect("republished map should read")
            .map
            .directories
            .iter()
            .any(|entry| entry.directory == "integrations")
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn directory_mutation_rejects_invalid_legacy_input_before_creating_migration_staging() {
    let (root, service, context) = legacy_v2_fixture("directory-mutation-preflight").await;
    let legacy_before = fs::read(service.legacy_map_path())
        .await
        .expect("legacy map should read");

    let error = service
        .add_directory(
            &context,
            RepositoryMapDirectory {
                directory: "../outside".to_owned(),
                purpose: "Invalid governed directory.".to_owned(),
                content_scope: vec!["knowledge/outside/**".to_owned()],
                key_files: vec!["knowledge/outside/README.md".to_owned()],
                load_hint: DirectoryLoadHint::OnDemand,
                relations: Vec::new(),
                update_rule: DirectoryUpdateRule::Reviewed,
            },
        )
        .await
        .expect_err("invalid legacy directory must fail before migration staging");

    assert!(
        error.to_string().contains("directory"),
        "the invalid directory should be rejected during preflight"
    );
    assert_eq!(
        fs::read(service.legacy_map_path())
            .await
            .expect("legacy map should remain visible"),
        legacy_before
    );
    assert!(
        !fs::try_exists(service.map_path())
            .await
            .expect("migration staging should be probed"),
        "invalid directory input must not create a forward-migration staging root"
    );
    assert!(
        !fs::try_exists(service.legacy_backup_path())
            .await
            .expect("migration backup should be probed"),
        "invalid directory input must not replace the rollback backup"
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rollback_rejects_an_unreadable_routed_legacy_glossary_before_hiding_v3() {
    let (root, service, context) = migrated_v2_fixture("rollback-routed-glossary-preflight").await;
    let visible_before = fs::read(service.map_path())
        .await
        .expect("visible v3 root should read");
    let mut legacy_backup = parse_manifest(
        &fs::read_to_string(service.legacy_backup_path())
            .await
            .expect("legacy backup should read"),
    )
    .expect("legacy backup should parse");
    let business_topic = legacy_backup
        .topics
        .iter_mut()
        .find(|topic| topic.id == "business-knowledge")
        .expect("legacy backup should route the business glossary");
    let mut shard: KnowledgeMapTopicShard = serde_norway::from_str(
        &fs::read_to_string(
            root.join(LEGACY_AGENT_CONTRACT_DIR_NAME)
                .join(&business_topic.r#ref),
        )
        .await
        .expect("legacy business shard should read"),
    )
    .expect("legacy business shard should parse");
    shard
        .sources
        .iter_mut()
        .find(|source| source.id == "repository-business-glossary")
        .expect("legacy business glossary source should exist")
        .uri = LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned();
    let shard_yaml = serialize_yaml(&shard).expect("legacy business shard should serialize");
    business_topic.digest = content_digest(shard_yaml.as_bytes());
    business_topic.r#ref = format!(
        "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{}.yaml",
        stable_id(&business_topic.id),
        business_topic.digest
    );
    fs::write(
        root.join(LEGACY_AGENT_CONTRACT_DIR_NAME)
            .join(&business_topic.r#ref),
        shard_yaml,
    )
    .await
    .expect("routed legacy business shard should write");
    fs::write(
        service.legacy_backup_path(),
        serialize_yaml(&legacy_backup).expect("legacy backup should serialize"),
    )
    .await
    .expect("legacy backup should write");

    let error = service
        .rollback_v3(&context)
        .await
        .expect_err("rollback must reject a missing routed legacy glossary");

    assert!(error.to_string().contains("No such file"));
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("failed rollback must keep the v3 root visible"),
        visible_before
    );
    assert!(
        !fs::try_exists(service.retained_v3_path())
            .await
            .expect("retained root should be probed"),
        "rollback must not move v3 data before the glossary preflight"
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn migration_refreshes_the_canonical_glossary_from_the_active_legacy_glossary() {
    let (root, service, context) = migrated_v2_fixture("refresh-legacy-glossary").await;

    service
        .rollback_v3(&context)
        .await
        .expect("rollback should restore the v2 root");
    let canonical_glossary = root.join(BUSINESS_GLOSSARY_RELATIVE_PATH);
    let canonical_before = fs::read(&canonical_glossary)
        .await
        .expect("canonical glossary should remain after rollback");
    let active_legacy_glossary =
        b"schema_version: 1\ndomains:\n  - id: sales\n    name: Sales\nterms: []\n";
    BusinessGlossary::parse(active_legacy_glossary)
        .expect("active legacy glossary fixture should validate");
    fs::write(
        root.join(LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH),
        active_legacy_glossary,
    )
    .await
    .expect("legacy writer should update its glossary");

    service
        .migrate_to_v3(&context)
        .await
        .expect("forward migration should refresh the canonical glossary");

    assert_ne!(canonical_before, active_legacy_glossary);
    assert_eq!(
        fs::read(&canonical_glossary)
            .await
            .expect("canonical glossary should read"),
        active_legacy_glossary
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn repeated_clean_rollback_preserves_active_legacy_edits() {
    let (root, service, context) = migrated_v2_fixture("repeated-clean-rollback").await;

    service
        .rollback_v3(&context)
        .await
        .expect("first rollback should restore the v2 root");
    let mut legacy = parse_manifest(
        &fs::read_to_string(service.legacy_map_path())
            .await
            .expect("active legacy root should read"),
    )
    .expect("active legacy root should parse");
    legacy.updated_at = "unix:2".to_owned();
    let edited_legacy = serialize_yaml(&legacy).expect("edited legacy root should serialize");
    fs::write(service.legacy_map_path(), &edited_legacy)
        .await
        .expect("legacy writer should update the active root");

    let response = service
        .rollback_v3(&context)
        .await
        .expect("repeated clean rollback should preserve the active legacy root");

    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("edited legacy root should remain active"),
        edited_legacy
    );
    assert!(response.summary.contains("already active"));
    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    assert!(fs::try_exists(service.retained_v3_path()).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn redirect_recovery_defers_legacy_redirect_until_visible_v3_publication() {
    let (root, service, _context) = legacy_v2_fixture("defer-legacy-redirect").await;
    let legacy = service.legacy_map_path();
    let legacy_before = fs::read(&legacy).await.expect("legacy root should read");

    service
        .prepare_legacy_migration()
        .await
        .expect("migration preparation should publish the v2 staging root");
    let visible_before = fs::read(service.map_path())
        .await
        .expect("visible staging root should read");
    assert!(visible_before.starts_with(b"schema_version: 2\n"));

    let error = service
        .publish_legacy_redirect()
        .await
        .expect_err("a v2 staging root must not publish the legacy redirect");
    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));

    service
        .recover_legacy_redirect_transition()
        .await
        .expect("redirect recovery should defer the incomplete v3 publication");

    assert_eq!(
        fs::read(&legacy)
            .await
            .expect("legacy root should remain readable"),
        legacy_before
    );
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("visible staging root should remain available for conversion"),
        visible_before
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rollback_discards_initial_forward_staging_and_keeps_the_live_legacy_root() {
    let (root, service, context) = legacy_v2_fixture("initial-forward-rollback").await;
    let legacy_before = fs::read(service.legacy_map_path())
        .await
        .expect("legacy root should read");

    service
        .prepare_legacy_migration()
        .await
        .expect("initial migration should publish only legacy staging");
    assert!(
        !service
            .validate_visible_v3_map_content(
                &fs::read_to_string(service.map_path())
                    .await
                    .expect("initial staging root should read"),
            )
            .await
            .expect("initial staging root should validate as incomplete")
    );
    assert!(
        !fs::try_exists(service.retained_v3_path())
            .await
            .expect("initial migration must not invent a retained v3 root")
    );

    let response = service
        .rollback_v3(&context)
        .await
        .expect("rollback should discard initial staging without a retained v3 root");

    assert_eq!(
        fs::read(service.legacy_map_path())
            .await
            .expect("live legacy root should remain readable"),
        legacy_before
    );
    assert!(
        !fs::try_exists(service.map_path())
            .await
            .expect("initial staging root should be removed")
    );
    assert!(response.summary.contains("initial migration staging"));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn redirect_recovery_preserves_legacy_edits_after_v3_publication() {
    let (root, service, context) =
        migrated_v2_fixture("preserve-post-publication-legacy-edits").await;
    let visible_before = fs::read(service.map_path())
        .await
        .expect("visible v3 root should read");
    let mut edited_legacy = parse_manifest(
        &fs::read_to_string(service.legacy_backup_path())
            .await
            .expect("migration backup should read"),
    )
    .expect("migration backup should parse");
    edited_legacy.updated_at = "unix:2".to_owned();
    let edited_legacy =
        serialize_yaml(&edited_legacy).expect("edited legacy root should serialize");
    fs::write(service.legacy_map_path(), &edited_legacy)
        .await
        .expect("legacy writer should update the live root after v3 publication");

    let error = service
        .init(&context)
        .await
        .expect_err("redirect recovery must preserve post-publication legacy edits");

    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("edited legacy root should remain live"),
        edited_legacy
    );
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("visible v3 root should remain available"),
        visible_before
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn redirect_recovery_preserves_legacy_glossary_edits_after_v3_publication() {
    let (root, service, context) =
        migrated_v2_fixture("preserve-post-publication-legacy-glossary-edits").await;
    let visible_before = fs::read(service.map_path())
        .await
        .expect("visible v3 root should read");
    let legacy_backup = fs::read(service.legacy_backup_path())
        .await
        .expect("legacy migration backup should read");
    let canonical_before = fs::read(root.join(BUSINESS_GLOSSARY_RELATIVE_PATH))
        .await
        .expect("canonical glossary should read");
    let edited_glossary =
        b"schema_version: 1\ndomains:\n  - id: sales\n    name: Sales\nterms: []\n";
    BusinessGlossary::parse(edited_glossary).expect("edited legacy glossary should validate");
    fs::write(service.legacy_map_path(), &legacy_backup)
        .await
        .expect("legacy writer should restore the map root before editing its glossary");
    fs::write(
        root.join(LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH),
        edited_glossary,
    )
    .await
    .expect("legacy writer should update only the glossary");

    let error = service
        .init(&context)
        .await
        .expect_err("redirect recovery must preserve a post-publication legacy glossary edit");

    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));
    assert!(error.to_string().contains("business glossary diverged"));
    assert_eq!(
        fs::read(service.legacy_map_path())
            .await
            .expect("legacy map should remain writable"),
        legacy_backup
    );
    assert_eq!(
        fs::read(root.join(BUSINESS_GLOSSARY_RELATIVE_PATH))
            .await
            .expect("canonical glossary should remain visible"),
        canonical_before
    );
    assert_eq!(
        fs::read(root.join(LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH))
            .await
            .expect("edited legacy glossary should remain visible"),
        edited_glossary
    );
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("visible v3 root should remain available"),
        visible_before
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn redirect_recovery_preserves_staged_legacy_edits_after_v3_publication() {
    let (root, service, context) =
        migrated_v2_fixture("preserve-staged-post-publication-legacy-edits").await;
    let visible_before = fs::read(service.map_path())
        .await
        .expect("visible v3 root should read");
    let redirect = fs::read(service.legacy_map_path())
        .await
        .expect("legacy redirect should read");
    let mut edited_legacy = parse_manifest(
        &fs::read_to_string(service.legacy_backup_path())
            .await
            .expect("migration backup should read"),
    )
    .expect("migration backup should parse");
    edited_legacy.updated_at = "unix:2".to_owned();
    let edited_legacy =
        serialize_yaml(&edited_legacy).expect("edited legacy root should serialize");
    fs::write(service.legacy_redirect_previous_path(), &edited_legacy)
        .await
        .expect("staged legacy root should preserve the old writer edit");
    fs::remove_file(service.legacy_map_path())
        .await
        .expect("interrupted redirect should leave no live legacy root");
    fs::write(service.legacy_redirect_prepared_path(), &redirect)
        .await
        .expect("interrupted redirect should retain the prepared redirect");

    let error = service
        .init(&context)
        .await
        .expect_err("redirect recovery must preserve staged post-publication legacy edits");

    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));
    assert_eq!(
        fs::read_to_string(service.legacy_redirect_previous_path())
            .await
            .expect("edited staged legacy root should remain available"),
        edited_legacy
    );
    assert_eq!(
        fs::read(service.legacy_redirect_prepared_path())
            .await
            .expect("prepared redirect should remain available"),
        redirect
    );
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("visible v3 root should remain available"),
        visible_before
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

    let snapshot = service
        .read_root_snapshot()
        .await
        .expect("retained v3 root should keep its selected contract directory");
    assert_eq!(snapshot.content, current_before);
    assert_eq!(snapshot.contract_dir, AGENT_CONTRACT_DIR_NAME);
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
async fn rollback_discards_incomplete_forward_staging_without_replacing_retained_v3() {
    let (root, service, context) = migrated_v2_fixture("incomplete-forward-rollback").await;
    service
        .rollback_v3(&context)
        .await
        .expect("clean rollback should retain the v3 root");
    let retained_before = fs::read_to_string(service.retained_v3_path())
        .await
        .expect("retained v3 root should read");

    service
        .prepare_legacy_migration()
        .await
        .expect("forward migration should stage the legacy root before publication");
    assert!(
        !service
            .validate_visible_v3_map_content(
                &fs::read_to_string(service.map_path())
                    .await
                    .expect("staged forward root should read"),
            )
            .await
            .expect("staged legacy root should not be v3")
    );

    let response = service
        .rollback_v3(&context)
        .await
        .expect("rollback should discard incomplete forward staging");

    assert_eq!(
        fs::read_to_string(service.retained_v3_path())
            .await
            .expect("retained v3 root should remain intact"),
        retained_before
    );
    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    assert!(response.summary.contains("already active"));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rollback_recovers_forward_staging_from_manifest_backup_before_discarding_it() {
    let (root, service, context) = migrated_v2_fixture("backup-forward-staging-rollback").await;
    service
        .rollback_v3(&context)
        .await
        .expect("clean rollback should retain the v3 root");
    let retained_before = fs::read(service.retained_v3_path())
        .await
        .expect("retained v3 root should read");

    service
        .prepare_legacy_migration()
        .await
        .expect("forward migration should stage the legacy root");
    fs::rename(service.map_path(), service.backup_path())
        .await
        .expect("interrupted manifest publication should retain staging as its backup");

    let response = service
        .rollback_v3(&context)
        .await
        .expect("rollback should recover and discard manifest-backup staging");

    assert_eq!(
        fs::read(service.retained_v3_path())
            .await
            .expect("retained v3 root should remain intact"),
        retained_before
    );
    assert!(
        !fs::try_exists(service.map_path()).await.unwrap(),
        "recovered staging must be discarded rather than treated as a visible v3 contract"
    );
    assert!(
        !fs::try_exists(service.backup_path()).await.unwrap(),
        "manifest backup staging must not be retained after rollback"
    );
    assert!(response.summary.contains("already active"));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn committed_rollback_recovery_keeps_post_commit_legacy_edits_when_cleanup_is_retried() {
    let (root, service, context) = migrated_v2_fixture("rollback-cleanup-residue").await;

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

    let response = service
        .rollback_v3(&context)
        .await
        .expect("repeated rollback should only clean committed rollback residue");

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
    assert!(response.summary.contains("retained committed"));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn successful_rollback_removes_stale_redirect_transition_markers() {
    let (root, service, context) = migrated_v2_fixture("rollback-cleans-redirect-markers").await;
    fs::write(
        service.legacy_redirect_previous_path(),
        fs::read(service.legacy_backup_path())
            .await
            .expect("legacy backup should read"),
    )
    .await
    .expect("stale redirect previous marker should write");
    fs::write(
        service.legacy_redirect_prepared_path(),
        "schema_version: 3\nartifact_kind: redirect\nmap_type: knowledge\ntarget: knowledge/knowledge-map.yaml\n",
    )
    .await
    .expect("stale redirect prepared marker should write");

    service
        .rollback_v3(&context)
        .await
        .expect("rollback should commit despite stale redirect markers");

    assert!(
        !fs::try_exists(service.legacy_redirect_prepared_path())
            .await
            .unwrap()
    );
    assert!(
        !fs::try_exists(service.legacy_redirect_previous_path())
            .await
            .unwrap()
    );
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
