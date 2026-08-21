use super::*;

#[tokio::test]
async fn writes_and_reads_yaml_contract() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-map-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos()
    ));
    fs::create_dir_all(&root).await.expect("root should create");
    fs::write(
        root.join("AGENTS.md"),
        format!("Knowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}"),
    )
    .await
    .expect("agents should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    service.init(&context).await.expect("init should work");
    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "build-cargo".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Config,
                uri: "Cargo.toml".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: None,
            },
        )
        .await
        .expect("source should add");
    service
        .update_source(
            &context,
            crate::domain::KnowledgeMapChange {
                id: "build-cargo".to_owned(),
                topic: None,
                kind: None,
                uri: None,
                source_scope: None,
                description: Some("Cargo package manifest".to_owned()),
            },
        )
        .await
        .expect("existing map should be replaceable");
    let route = service
        .route(&context, "build".to_owned())
        .await
        .expect("route should load");
    let validation = service
        .validate(&context)
        .await
        .expect("validate should run");

    assert_eq!(route.sources[0].id, "build-cargo");
    assert!(validation.valid);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn concurrent_source_adds_preserve_both_changes() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-map-concurrent-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos()
    ));
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");

    let first = service.add_source(
        &context,
        KnowledgeMapSourceAddRequest {
            id: "build-cargo".to_owned(),
            topic: "build".to_owned(),
            kind: KnowledgeMapSourceKind::Config,
            uri: "Cargo.toml".to_owned(),
            source_scope: Some("repo".to_owned()),
            description: None,
        },
    );
    let second = service.add_source(
        &context,
        KnowledgeMapSourceAddRequest {
            id: "build-readme".to_owned(),
            topic: "build".to_owned(),
            kind: KnowledgeMapSourceKind::Doc,
            uri: "README.md".to_owned(),
            source_scope: Some("repo".to_owned()),
            description: None,
        },
    );

    let (first, second) = tokio::join!(first, second);
    first.expect("first add should succeed");
    second.expect("second add should succeed");
    let route = service
        .route(&context, "build".to_owned())
        .await
        .expect("route should load");

    assert_eq!(route.sources.len(), 2);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn init_upgrades_legacy_map_once() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-map-upgrade-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos()
    ));
    fs::create_dir_all(root.join(AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("knowledge directory should create");
    let mut legacy = KnowledgeMap::initial("legacy".to_owned());
    legacy
        .remove_source("repository-software-model")
        .expect("legacy fixture should omit the new default route");
    legacy.schema_version = 1;
    let legacy_yaml = serde_norway::to_string(&legacy).expect("legacy map should serialize");
    fs::write(root.join(KNOWLEDGE_MAP_RELATIVE_PATH), legacy_yaml)
        .await
        .expect("legacy map should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    let upgraded = service.init(&context).await.expect("upgrade should work");
    let repeated = service
        .init(&context)
        .await
        .expect("repeat init should be idempotent");
    let shown = service
        .show(&context, Some("software-model".to_owned()))
        .await
        .expect("upgraded map should load");

    assert_eq!(upgraded.map_version, 2);
    assert_eq!(repeated.map_version, upgraded.map_version);
    assert_eq!(shown.map.sources.len(), 1);
    assert_eq!(shown.map.sources[0].id, "repository-software-model");
    assert_eq!(shown.map.history.last().expect("history").version, 2);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn route_loads_only_the_requested_topic_shard() {
    let root = temp_root("progressive");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    for (id, topic) in [("cargo", "build"), ("adr", "architecture")] {
        service
            .add_source(
                &context,
                KnowledgeMapSourceAddRequest {
                    id: id.to_owned(),
                    topic: topic.to_owned(),
                    kind: KnowledgeMapSourceKind::Doc,
                    uri: format!("docs/{id}.md"),
                    source_scope: Some("repo".to_owned()),
                    description: None,
                },
            )
            .await
            .expect("source should add");
    }
    let manifest_text = fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
        .await
        .expect("manifest should read");
    let manifest = parse_manifest(&manifest_text).expect("manifest should parse");
    let unrelated = manifest
        .topics
        .iter()
        .find(|topic| topic.id == "architecture")
        .expect("architecture topic");
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(&unrelated.r#ref),
        "corrupted",
    )
    .await
    .expect("unrelated shard should corrupt");

    let route = service
        .route(&context, "build".to_owned())
        .await
        .expect("build route should not load architecture");
    assert_eq!(route.sources[0].id, "cargo");
    assert!(!service.validate(&context).await.expect("validate").valid);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn bounds_recent_history_and_detects_archive_tampering() {
    let root = temp_root("history");
    fs::create_dir_all(&root).await.expect("root should create");
    fs::write(
        root.join("AGENTS.md"),
        format!("Knowledge map: {KNOWLEDGE_MAP_RELATIVE_PATH}"),
    )
    .await
    .expect("agents should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    for index in 0..RECENT_HISTORY_LIMIT * 2 + 2 {
        service
            .add_source(
                &context,
                KnowledgeMapSourceAddRequest {
                    id: format!("source-{index}"),
                    topic: "build".to_owned(),
                    kind: KnowledgeMapSourceKind::Config,
                    uri: format!("build/{index}.toml"),
                    source_scope: Some("repo".to_owned()),
                    description: None,
                },
            )
            .await
            .expect("source should add");
    }
    let manifest_text = fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
        .await
        .expect("manifest should read");
    let manifest = parse_manifest(&manifest_text).expect("manifest should parse");
    assert_eq!(manifest.history.recent.len(), 3);
    assert_eq!(
        manifest.history.archived_through,
        (RECENT_HISTORY_LIMIT * 2) as u64
    );
    let archive_ref = manifest.history.archive.expect("archive should exist");
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(archive_ref.r#ref),
        "tampered",
    )
    .await
    .expect("archive should tamper");
    assert!(!service.validate(&context).await.expect("validate").valid);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rejects_topic_refs_that_escape_the_contract_directory() {
    let root = temp_root("unsafe-ref");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    let manifest_text = fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
        .await
        .expect("manifest should read");
    let mut manifest = parse_manifest(&manifest_text).expect("manifest should parse");
    manifest.topics[0].r#ref = "topics/../outside.yaml".to_owned();
    fs::write(
        root.join(KNOWLEDGE_MAP_RELATIVE_PATH),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let error = service
        .route(&context, "software-model".to_owned())
        .await
        .expect_err("unsafe ref must fail");
    assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
    let _ = fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_topic_directory_symlink_that_escapes_the_repository() {
    use std::os::unix::fs::symlink;

    let root = temp_root("unsafe-write-symlink");
    let outside = temp_root("unsafe-write-target");
    fs::create_dir_all(root.join(AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("contract directory should create");
    fs::create_dir_all(&outside)
        .await
        .expect("outside directory should create");
    symlink(
        &outside,
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME),
    )
    .expect("topic directory symlink should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    let error = service
        .init(&context)
        .await
        .expect_err("artifact publication must reject a symlink escape");
    assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
    assert!(
        fs::read_dir(&outside)
            .await
            .expect("outside directory should remain readable")
            .next_entry()
            .await
            .expect("outside directory listing should work")
            .is_none()
    );
    let _ = fs::remove_dir_all(root).await;
    let _ = fs::remove_dir_all(outside).await;
}

#[tokio::test]
async fn recovers_a_root_manifest_left_in_the_publish_backup() {
    let root = temp_root("publish-recovery");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    fs::rename(service.map_path(), service.backup_path())
        .await
        .expect("interrupted publish should leave a backup");

    service
        .route(&context, "software-model".to_owned())
        .await
        .expect("read path should fall back to the previous root");
    service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "build".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Config,
                uri: "Cargo.toml".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: None,
            },
        )
        .await
        .expect("writer should recover before mutation");
    assert!(
        fs::try_exists(service.map_path())
            .await
            .expect("root check")
    );
    assert!(
        !fs::try_exists(service.backup_path())
            .await
            .expect("backup check")
    );
    let _ = fs::remove_dir_all(root).await;
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "relay-knowledge-map-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos()
    ))
}
