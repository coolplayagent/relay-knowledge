use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::{
    api::{InterfaceKind, RequestContext},
    domain::{KnowledgeMap, KnowledgeMapChange, KnowledgeMapSourceKind},
};
use tokio::time::Duration;

use super::super::{
    KnowledgeMapSourceAddRequest, cleanup_superseded_topic_shards,
    cleanup_superseded_topic_shards_in, parse_manifest,
};

#[tokio::test]
async fn legacy_map_migrates_to_visible_v4_and_legacy_recovery_stays_readable() {
    let root = temp_root("map-v3-migration");
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy directory should create");
    fs::write(
        root.join("AGENTS.md"),
        "CodeSpec map: codespec/codespec-map.yaml\nKnowledge map: knowledge/knowledge-map.yaml\n",
    )
    .await
    .expect("agents should write");
    let legacy = KnowledgeMap::initial("unix:1".to_owned());
    let legacy_yaml = serde_norway::to_string(&legacy).expect("legacy map should serialize");
    fs::write(root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH), &legacy_yaml)
        .await
        .expect("legacy map should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    service
        .migrate_to_v4(&context)
        .await
        .expect("migration should work");
    let visible = fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
        .await
        .expect("visible root should exist");
    assert!(visible.contains("schema_version: 4"));
    assert!(
        fs::try_exists(service.backup_path()).await.unwrap(),
        "v3 publication should leave the ordinary reader fallback used by this regression"
    );
    let redirect = fs::read_to_string(root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH))
        .await
        .expect("legacy redirect should exist");
    assert!(redirect.contains("artifact_kind: redirect"));

    service
        .rollback_v3(&context)
        .await
        .expect("rollback should work");
    assert!(
        !fs::try_exists(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
            .await
            .expect("visible root should be probed")
    );
    let restored = fs::read_to_string(root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH))
        .await
        .expect("legacy root should restore");
    assert!(restored.contains("schema_version: 1"));
    assert_eq!(restored, legacy_yaml, "rollback must restore exact bytes");
    assert!(
        !fs::try_exists(service.backup_path()).await.unwrap(),
        "the ordinary visible backup must not shadow the restored legacy root"
    );
    assert!(
        fs::try_exists(service.retained_v3_backup_path())
            .await
            .unwrap(),
        "the ordinary backup should remain available outside the reader fallback"
    );
    assert_eq!(
        service
            .read_contract_dir_name()
            .await
            .expect("contract directory should resolve"),
        LEGACY_AGENT_CONTRACT_DIR_NAME
    );
    assert_eq!(
        service
            .read_root_content()
            .await
            .expect("active root should read"),
        legacy_yaml
    );

    let shown = service
        .show(&context, None)
        .await
        .expect("show should read the restored legacy root");
    assert_eq!(
        shown.map.artifact_schema_version,
        KnowledgeMap::SCHEMA_VERSION
    );
    assert_eq!(shown.map.updated_at, "unix:1");
    let routed = service
        .route(&context, "software-model".to_owned())
        .await
        .expect("route should read the restored legacy root");
    assert_eq!(
        routed
            .route
            .expect("legacy software route should resolve")
            .source_order,
        ["repository-software-model"]
    );
    assert_eq!(routed.sources.len(), 1);
    assert_eq!(routed.sources[0].id, "repository-software-model");

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn legacy_map_without_current_reserved_routes_migrates_and_remains_rollbackable() {
    let root = temp_root("map-v3-legacy-without-builtins");
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy directory should create");
    let mut legacy = KnowledgeMap::initial("unix:1".to_owned());
    legacy.topics.clear();
    legacy.sources.clear();
    legacy.routes.clear();
    legacy
        .validate()
        .expect("legacy map without current reserved routes remains structurally valid");
    let legacy_yaml = serde_norway::to_string(&legacy).expect("legacy map should serialize");
    fs::write(root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH), &legacy_yaml)
        .await
        .expect("legacy map should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    service
        .migrate_to_v4(&context)
        .await
        .expect("current migration should add reserved routes to the visible contract");
    service
        .validate_map_contract()
        .await
        .expect("visible contract should satisfy current reserved-route rules");
    assert_eq!(
        fs::read_to_string(service.legacy_backup_path())
            .await
            .expect("legacy backup should read"),
        legacy_yaml,
        "migration must retain the exact old recovery boundary"
    );

    service
        .rollback_v3(&context)
        .await
        .expect("legacy structural validation should permit rollback");
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("rolled-back legacy root should read"),
        legacy_yaml
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn migration_repairs_legacy_reserved_sources_without_route_membership() {
    let root = temp_root("map-v3-legacy-repairs-reserved-routes");
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy directory should create");
    let mut legacy = KnowledgeMap::initial("unix:1".to_owned());
    for topic in ["software-model", "business-knowledge"] {
        legacy
            .routes
            .iter_mut()
            .find(|route| route.topic == topic)
            .expect("reserved route should exist")
            .source_order
            .clear();
    }
    let legacy_yaml =
        serde_norway::to_string(&legacy).expect("repairable legacy map should serialize");
    fs::write(root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH), &legacy_yaml)
        .await
        .expect("repairable legacy map should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    service
        .migrate_to_v4(&context)
        .await
        .expect("migration should repair reserved route membership");
    service
        .validate_map_contract()
        .await
        .expect("published v3 map should satisfy reserved route validation");
    for (topic, source) in [
        ("software-model", "repository-software-model"),
        ("business-knowledge", "repository-business-glossary"),
    ] {
        let route = service
            .route(&context, topic.to_owned())
            .await
            .expect("migrated route should load")
            .route
            .expect("reserved route should remain visible");
        assert!(route.source_order.iter().any(|entry| entry == source));
    }
    service
        .rollback_v3(&context)
        .await
        .expect("repairable legacy sources should remain rollbackable");
    assert_eq!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("rolled-back legacy root should read"),
        legacy_yaml
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn migration_rejects_a_legacy_redirect_instead_of_copying_it_as_a_visible_root() {
    let root = temp_root("legacy-redirect-is-not-a-map");
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy directory should create");
    fs::write(
        root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH),
        "schema_version: 4\nartifact_kind: redirect\nmap_type: knowledge\ntarget: knowledge/knowledge-map.yaml\n",
    )
    .await
    .expect("orphan redirect should seed");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    service
        .init(&context)
        .await
        .expect_err("an orphan redirect must not be copied as a visible map");

    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    assert!(!fs::try_exists(service.legacy_backup_path()).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn migration_rejects_legacy_tree_directory_and_entry_symlinks() {
    use std::os::unix::fs::symlink;

    for symlink_directory in [true, false] {
        let label = if symlink_directory {
            "legacy-tree-directory-symlink"
        } else {
            "legacy-tree-entry-symlink"
        };
        let root = temp_root(label);
        let outside = temp_root(&format!("{label}-outside"));
        let legacy_dir = root.join(LEGACY_AGENT_CONTRACT_DIR_NAME);
        let legacy_topics = legacy_dir.join(KNOWLEDGE_MAP_TOPICS_DIR_NAME);
        fs::create_dir_all(&legacy_dir)
            .await
            .expect("legacy directory should create");
        fs::create_dir_all(&outside)
            .await
            .expect("outside directory should create");
        let outside_file = outside.join("outside.yaml");
        let outside_before = b"outside must not be copied through a symlink";
        fs::write(&outside_file, outside_before)
            .await
            .expect("outside file should write");
        if symlink_directory {
            symlink(&outside, &legacy_topics).expect("topic directory symlink should create");
        } else {
            fs::create_dir_all(&legacy_topics)
                .await
                .expect("legacy topics directory should create");
            symlink(&outside_file, legacy_topics.join("unreferenced.yaml"))
                .expect("topic entry symlink should create");
        }
        let legacy = KnowledgeMap::initial("unix:1".to_owned());
        fs::write(
            root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH),
            serde_norway::to_string(&legacy).expect("legacy map should serialize"),
        )
        .await
        .expect("legacy root should write");
        let service = KnowledgeMapService::new(root.clone());
        let context = RequestContext::for_interface(InterfaceKind::Cli);

        let error = service
            .migrate_to_v4(&context)
            .await
            .expect_err("legacy tree symlinks must fail closed");

        assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
        assert!(!fs::try_exists(service.map_path()).await.unwrap());
        assert_eq!(
            fs::read(&outside_file)
                .await
                .expect("outside file should remain readable"),
            outside_before
        );
        let _ = fs::remove_dir_all(root).await;
        let _ = fs::remove_dir_all(outside).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn rollback_rejects_a_symlink_destination_before_moving_the_visible_root() {
    use std::os::unix::fs::symlink;

    let (root, service, context) = migrated_v2_fixture("rollback-symlink-destination").await;
    let outside = temp_root("rollback-symlink-outside");
    fs::create_dir_all(&outside)
        .await
        .expect("outside directory should create");
    let outside_file = outside.join("outside.yaml");
    let outside_before = b"outside must remain unchanged";
    fs::write(&outside_file, outside_before)
        .await
        .expect("outside file should write");
    fs::remove_file(service.legacy_map_path())
        .await
        .expect("legacy redirect should remove");
    symlink(&outside_file, service.legacy_map_path())
        .expect("legacy destination symlink should create");
    let visible_before = fs::read(service.map_path())
        .await
        .expect("visible root should read");

    let error = service
        .rollback_v3(&context)
        .await
        .expect_err("rollback destination symlink must fail closed");

    assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("visible root should remain"),
        visible_before
    );
    assert_eq!(
        fs::read(&outside_file)
            .await
            .expect("outside file should remain readable"),
        outside_before
    );
    assert!(!fs::try_exists(service.retained_v3_path()).await.unwrap());
    assert!(
        !fs::try_exists(service.legacy_rollback_prepared_path())
            .await
            .unwrap()
    );
    let _ = fs::remove_dir_all(root).await;
    let _ = fs::remove_dir_all(outside).await;
}

#[tokio::test]
async fn v2_reader_resolves_shards_from_the_legacy_contract_root() {
    let root = temp_root("map-v2-reader-root");
    fs::create_dir_all(&root)
        .await
        .expect("repository root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    service
        .init(&context)
        .await
        .expect("v3 map should initialize");
    fs::remove_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("current writer lock directory should clear before seeding a v2 root");
    fs::rename(
        root.join(AGENT_CONTRACT_DIR_NAME),
        root.join(LEGACY_AGENT_CONTRACT_DIR_NAME),
    )
    .await
    .expect("contract should move to the legacy root");
    let legacy_root = root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH);
    let manifest = fs::read_to_string(&legacy_root)
        .await
        .expect("legacy root should read")
        .replacen("schema_version: 4", "schema_version: 2", 1)
        .replacen("omitted_through", "archived_through", 1);
    fs::write(legacy_root, manifest)
        .await
        .expect("v2 root should write");

    service
        .validate_map_contract()
        .await
        .expect("v2 refs should resolve beside the legacy root");
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rollback_rejects_a_missing_backup_without_moving_the_visible_root() {
    let (root, service, context) = migrated_v2_fixture("rollback-missing-backup").await;
    let visible_before = fs::read(service.map_path())
        .await
        .expect("visible v3 root should read");
    fs::remove_file(service.legacy_backup_path())
        .await
        .expect("rollback backup should remove");

    let error = service
        .rollback_v3(&context)
        .await
        .expect_err("missing rollback backup must fail");

    assert!(matches!(error, KnowledgeMapServiceError::InvalidRequest(_)));
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("visible root should remain"),
        visible_before
    );
    assert!(!fs::try_exists(service.retained_v3_path()).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rollback_fully_validates_the_v2_backup_before_moving_the_visible_root() {
    let (root, service, context) = migrated_v2_fixture("rollback-corrupt-backup").await;
    let visible_before = fs::read(service.map_path())
        .await
        .expect("visible v3 root should read");
    let backup = parse_manifest(
        &fs::read_to_string(service.legacy_backup_path())
            .await
            .expect("v2 backup should read"),
    )
    .expect("v2 backup should parse");
    let shard = backup
        .topics
        .first()
        .expect("backup should reference a topic");
    fs::write(
        root.join(LEGACY_AGENT_CONTRACT_DIR_NAME).join(&shard.r#ref),
        "corrupt shard",
    )
    .await
    .expect("retained shard should corrupt");

    let error = service
        .rollback_v3(&context)
        .await
        .expect_err("corrupt retained graph must fail rollback preflight");

    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));
    assert_eq!(
        fs::read(service.map_path())
            .await
            .expect("visible root should remain"),
        visible_before
    );
    assert!(!fs::try_exists(service.retained_v3_path()).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn init_converges_each_legacy_redirect_crash_residue() {
    for residue in [
        RedirectResidue::PreparedAndPrevious,
        RedirectResidue::PreviousOnly,
        RedirectResidue::PreparedOnly,
    ] {
        let (root, service, context) = migrated_v2_fixture(residue.label()).await;
        let legacy = service.legacy_map_path();
        let prepared = service.legacy_redirect_prepared_path();
        let previous = service.legacy_redirect_previous_path();
        let backup = service.legacy_backup_path();
        let backup_before = fs::read(&backup).await.expect("v2 backup should read");
        let redirect = fs::read(&legacy).await.expect("redirect should read");

        match residue {
            RedirectResidue::PreparedAndPrevious => {
                fs::write(&prepared, &redirect)
                    .await
                    .expect("prepared redirect should seed");
                fs::copy(&backup, &previous)
                    .await
                    .expect("previous v2 root should seed");
                fs::remove_file(&legacy)
                    .await
                    .expect("live redirect should disappear");
            }
            RedirectResidue::PreviousOnly => {
                fs::copy(&backup, &previous)
                    .await
                    .expect("previous v2 root should seed");
                fs::remove_file(&legacy)
                    .await
                    .expect("live redirect should disappear");
            }
            RedirectResidue::PreparedOnly => {
                fs::copy(&backup, &legacy)
                    .await
                    .expect("live v2 root should seed");
                fs::write(&prepared, &redirect)
                    .await
                    .expect("prepared redirect should seed");
            }
        }

        service
            .init(&context)
            .await
            .expect("restart should converge the redirect publication");

        assert_eq!(
            fs::read(&legacy).await.expect("redirect should recover"),
            redirect
        );
        assert_eq!(
            fs::read(&backup).await.expect("v2 backup should remain"),
            backup_before
        );
        assert!(!fs::try_exists(prepared).await.unwrap());
        assert!(!fs::try_exists(previous).await.unwrap());
        let _ = fs::remove_dir_all(root).await;
    }
}

#[tokio::test]
async fn init_recovers_each_incomplete_rollback_transition_without_losing_either_root() {
    for residue in [
        RollbackResidue::CurrentRoot,
        RollbackResidue::BackupFallback,
        RollbackResidue::LegacyRedirect,
    ] {
        let (root, service, context) = migrated_v2_fixture(residue.label()).await;
        let current = service.map_path();
        let ordinary_backup = service.backup_path();
        let legacy = service.legacy_map_path();
        let legacy_backup = service.legacy_backup_path();
        let current_before = fs::read(&current).await.expect("v3 root should read");
        let ordinary_backup_before = fs::read(&ordinary_backup)
            .await
            .expect("ordinary backup should read");
        let redirect_before = fs::read(&legacy).await.expect("redirect should read");
        let legacy_backup_before = fs::read(&legacy_backup)
            .await
            .expect("legacy backup should read");
        fs::copy(&legacy_backup, service.legacy_rollback_prepared_path())
            .await
            .expect("rollback prepared root should seed");
        fs::rename(&current, service.retained_v3_path())
            .await
            .expect("current root should move");
        if residue.moves_ordinary_backup() {
            fs::rename(&ordinary_backup, service.retained_v3_backup_path())
                .await
                .expect("ordinary backup should move");
        }
        if residue.moves_legacy() {
            fs::rename(&legacy, service.legacy_rollback_previous_path())
                .await
                .expect("legacy redirect should move");
        }

        service
            .init(&context)
            .await
            .expect("restart should restore the pre-commit v3 state");

        assert_eq!(
            fs::read(&current).await.expect("v3 root should recover"),
            current_before
        );
        assert_eq!(
            fs::read(&ordinary_backup)
                .await
                .expect("ordinary backup should recover"),
            ordinary_backup_before
        );
        assert_eq!(
            fs::read(&legacy).await.expect("redirect should recover"),
            redirect_before
        );
        assert_eq!(
            fs::read(&legacy_backup)
                .await
                .expect("legacy backup should remain"),
            legacy_backup_before
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
}

#[tokio::test]
async fn committed_rollback_recovery_preserves_legacy_until_explicit_forward_init() {
    let (root, service, context) = migrated_v2_fixture("rollback-committed-before-cleanup").await;
    let current_before = fs::read(service.map_path())
        .await
        .expect("v3 root should read");
    let ordinary_backup_before = fs::read(service.backup_path())
        .await
        .expect("ordinary backup should read");
    let legacy_backup = fs::read(service.legacy_backup_path())
        .await
        .expect("legacy backup should read");
    let legacy_yaml =
        String::from_utf8(legacy_backup.clone()).expect("legacy backup should be UTF-8");
    let legacy_manifest = parse_manifest(&legacy_yaml).expect("legacy backup should parse");
    fs::copy(
        service.legacy_backup_path(),
        service.legacy_rollback_prepared_path(),
    )
    .await
    .expect("rollback prepared root should seed");
    fs::rename(service.map_path(), service.retained_v3_path())
        .await
        .expect("v3 root should retain");
    fs::rename(service.backup_path(), service.retained_v3_backup_path())
        .await
        .expect("ordinary backup should retain");
    fs::rename(
        service.legacy_map_path(),
        service.legacy_rollback_previous_path(),
    )
    .await
    .expect("legacy redirect should move");
    fs::rename(
        service.legacy_rollback_prepared_path(),
        service.legacy_map_path(),
    )
    .await
    .expect("legacy root publication is the rollback commit point");

    {
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
                .expect("restart recovery should finish the committed rollback")
        );
    }

    assert!(!fs::try_exists(service.map_path()).await.unwrap());
    assert!(!fs::try_exists(service.backup_path()).await.unwrap());
    assert_eq!(
        fs::read(service.retained_v3_path())
            .await
            .expect("retained v3 root should remain"),
        current_before
    );
    assert_eq!(
        fs::read(service.retained_v3_backup_path())
            .await
            .expect("retained ordinary backup should remain"),
        ordinary_backup_before
    );
    assert_eq!(
        fs::read(service.legacy_map_path())
            .await
            .expect("legacy root should remain active"),
        legacy_backup
    );
    assert!(
        !fs::try_exists(service.legacy_rollback_previous_path())
            .await
            .unwrap()
    );
    assert_eq!(
        service
            .read_contract_dir_name()
            .await
            .expect("legacy contract should resolve"),
        LEGACY_AGENT_CONTRACT_DIR_NAME
    );
    assert_eq!(
        service
            .read_root_content()
            .await
            .expect("active root should read"),
        legacy_yaml
    );
    let shown = service
        .show(&context, None)
        .await
        .expect("show should read the committed legacy root");
    assert_eq!(shown.map.updated_at, legacy_manifest.updated_at);
    let routed = service
        .route(&context, "software-model".to_owned())
        .await
        .expect("route should read the committed legacy root");
    assert_eq!(
        routed
            .route
            .expect("legacy route should resolve")
            .source_order,
        ["repository-software-model"]
    );

    service
        .init(&context)
        .await
        .expect("an explicit init should migrate forward after recovery");
    assert!(fs::try_exists(service.map_path()).await.unwrap());
    assert!(
        fs::read_to_string(service.legacy_map_path())
            .await
            .expect("legacy redirect should republish")
            .contains("artifact_kind: redirect")
    );
    assert_eq!(
        fs::read(service.retained_v3_path())
            .await
            .expect("retained v3 root should survive forward migration"),
        current_before
    );
    assert_eq!(
        fs::read(service.retained_v3_backup_path())
            .await
            .expect("retained backup should survive forward migration"),
        ordinary_backup_before
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn shard_cleanup_retains_refs_from_both_v3_rollback_roots() {
    let root = temp_root("rollback-shard-retention");
    fs::create_dir_all(&root)
        .await
        .expect("repository root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    service.init(&context).await.expect("map should initialize");
    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "build".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Config,
                uri: "Cargo.toml".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: Some("initial build route".to_owned()),
            },
        )
        .await
        .expect("source should add");
    let retained_root = fs::read_to_string(service.map_path())
        .await
        .expect("retained root should read");
    let retained_manifest = parse_manifest(&retained_root).expect("retained root should parse");
    let retired_ref = retained_manifest
        .topics
        .iter()
        .find(|topic| topic.id == "build")
        .expect("build topic should exist")
        .r#ref
        .clone();
    let retired_shard = root.join(AGENT_CONTRACT_DIR_NAME).join(retired_ref);
    service
        .update_source(
            &context,
            KnowledgeMapChange {
                id: "build".to_owned(),
                topic: None,
                kind: None,
                uri: None,
                source_scope: None,
                description: Some("updated build route".to_owned()),
            },
        )
        .await
        .expect("source should update");
    fs::remove_file(service.backup_path())
        .await
        .expect("ordinary recovery root should remove");
    fs::write(service.retained_v3_path(), &retained_root)
        .await
        .expect("rollback root should seed");
    let current = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("current root should read"),
    )
    .expect("current root should parse");

    cleanup_superseded_topic_shards(&root, &service.backup_path(), &current, Duration::ZERO).await;
    assert!(
        fs::try_exists(&retired_shard).await.unwrap(),
        "the rollback root must retain its unique shard"
    );

    fs::remove_file(service.retained_v3_path())
        .await
        .expect("rollback root should remove");
    fs::write(service.retained_v3_backup_path(), retained_root)
        .await
        .expect("rollback backup root should seed");
    cleanup_superseded_topic_shards(&root, &service.backup_path(), &current, Duration::ZERO).await;
    assert!(
        fs::try_exists(&retired_shard).await.unwrap(),
        "the rollback-retained ordinary backup must retain its unique shard"
    );

    fs::remove_file(service.retained_v3_backup_path())
        .await
        .expect("rollback backup root should remove");
    cleanup_superseded_topic_shards(&root, &service.backup_path(), &current, Duration::ZERO).await;
    assert!(!fs::try_exists(retired_shard).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn legacy_namespace_cleanup_retains_refs_from_the_v2_recovery_manifest() {
    let (root, service, _context) = migrated_v2_fixture("legacy-recovery-shard-retention").await;
    let backup_path = service.legacy_backup_path();
    let backup = parse_manifest(
        &fs::read_to_string(&backup_path)
            .await
            .expect("legacy backup should read"),
    )
    .expect("legacy backup should parse");
    let unique_topic = backup
        .topics
        .first()
        .expect("legacy backup should reference a topic");
    let unique_shard = root
        .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
        .join(&unique_topic.r#ref);
    let mut current = backup.clone();
    current.topics.remove(0);
    let absent_ordinary_backup = root
        .join(LEGACY_AGENT_CONTRACT_DIR_NAME)
        .join("absent.previous");

    cleanup_superseded_topic_shards_in(
        &root,
        LEGACY_AGENT_CONTRACT_DIR_NAME,
        &absent_ordinary_backup,
        &current,
        Duration::ZERO,
    )
    .await;
    assert!(
        fs::try_exists(&unique_shard).await.unwrap(),
        "the legacy v2 recovery manifest must retain its unique shard"
    );

    fs::remove_file(backup_path)
        .await
        .expect("legacy recovery manifest should remove");
    cleanup_superseded_topic_shards_in(
        &root,
        LEGACY_AGENT_CONTRACT_DIR_NAME,
        &absent_ordinary_backup,
        &current,
        Duration::ZERO,
    )
    .await;
    assert!(!fs::try_exists(unique_shard).await.unwrap());
    let _ = fs::remove_dir_all(root).await;
}

#[derive(Clone, Copy)]
enum RedirectResidue {
    PreparedAndPrevious,
    PreviousOnly,
    PreparedOnly,
}

#[derive(Clone, Copy)]
enum RollbackResidue {
    CurrentRoot,
    BackupFallback,
    LegacyRedirect,
}

impl RollbackResidue {
    fn label(self) -> &'static str {
        match self {
            Self::CurrentRoot => "rollback-current-moved",
            Self::BackupFallback => "rollback-ordinary-backup-moved",
            Self::LegacyRedirect => "rollback-legacy-moved",
        }
    }

    fn moves_ordinary_backup(self) -> bool {
        matches!(self, Self::BackupFallback | Self::LegacyRedirect)
    }

    fn moves_legacy(self) -> bool {
        matches!(self, Self::LegacyRedirect)
    }
}

impl RedirectResidue {
    fn label(self) -> &'static str {
        match self {
            Self::PreparedAndPrevious => "redirect-prepared-previous",
            Self::PreviousOnly => "redirect-previous-only",
            Self::PreparedOnly => "redirect-prepared-only",
        }
    }
}

async fn migrated_v2_fixture(label: &str) -> (PathBuf, KnowledgeMapService, RequestContext) {
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
    fs::remove_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("current writer lock directory should clear before seeding a v2 root");
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
        .replacen("schema_version: 4", "schema_version: 2", 1)
        .replacen("omitted_through", "archived_through", 1);
    fs::write(&legacy, v2)
        .await
        .expect("v2 fixture root should write");
    service
        .migrate_to_v4(&context)
        .await
        .expect("v2 fixture should migrate");
    (root, service, context)
}

#[test]
fn rollback_response_version_reads_a_legacy_manifest_without_a_history_index() {
    let legacy_manifest = r#"
schema_version: 2
map_version: 7
updated_at: unix:7
topics: []
history:
  archived_through: 6
  archive:
    ref: history/00000000000000000001-00000000000000000006-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.yaml
    digest: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  recent:
    - version: 7
      action: mutation
      actor: cli
      summary: retained legacy fixture
"#;

    assert_eq!(
        map_version_from_validated_legacy_content(legacy_manifest)
            .expect("a validated legacy root should expose its response version"),
        7
    );
}

fn temp_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("relay-knowledge-{name}-{nonce}"))
}
