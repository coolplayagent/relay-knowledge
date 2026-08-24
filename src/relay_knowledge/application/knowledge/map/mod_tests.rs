use super::*;
use crate::domain::KnowledgeMapSourceKind;

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
    let shown = service
        .show(&context, None)
        .await
        .expect("assembled map should load");
    let inline = serialize_yaml(&shown.map).expect("assembled map should serialize");
    assert_eq!(
        serde_norway::from_str::<KnowledgeMapSchemaProbe>(&inline)
            .expect("inline schema should parse")
            .schema_version,
        KnowledgeMap::SCHEMA_VERSION
    );
    fs::write(service.map_path(), inline)
        .await
        .expect("inline map should replace the root");
    assert_eq!(
        service
            .show(&context, None)
            .await
            .expect("serialized public map must remain readable")
            .map,
        shown.map
    );
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
async fn readers_retry_across_concurrent_manifest_publications() {
    let root = temp_root("concurrent-readers");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
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
        .expect("source should add");
    let writer = async {
        for index in 0..20 {
            service
                .update_source(
                    &context,
                    KnowledgeMapChange {
                        id: "build".to_owned(),
                        topic: None,
                        kind: None,
                        uri: None,
                        source_scope: None,
                        description: Some(format!("revision {index}")),
                    },
                )
                .await
                .expect("publication should succeed");
        }
    };
    let reader = async {
        for _ in 0..100 {
            let route = service
                .route(&context, "build".to_owned())
                .await
                .expect("reader should survive the rename window");
            assert_eq!(route.sources.len(), 1);
            tokio::task::yield_now().await;
        }
    };
    tokio::join!(writer, reader);
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
async fn route_rejects_a_digest_consistent_but_semantically_invalid_shard() {
    let root = temp_root("invalid-shard");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    let mut manifest = parse_manifest(
        &fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    let topic_ref = manifest
        .topics
        .iter_mut()
        .find(|topic| topic.id == "software-model")
        .expect("software topic should exist");
    let mut shard: KnowledgeMapTopicShard = serde_norway::from_str(
        &fs::read_to_string(root.join(AGENT_CONTRACT_DIR_NAME).join(&topic_ref.r#ref))
            .await
            .expect("shard should read"),
    )
    .expect("shard should parse");
    shard.sources[0].version = 0;
    let yaml = serialize_yaml(&shard).expect("invalid shard should serialize");
    topic_ref.digest = content_digest(yaml.as_bytes());
    topic_ref.r#ref = format!(
        "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{}.yaml",
        stable_id(&topic_ref.id),
        topic_ref.digest
    );
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(&topic_ref.r#ref),
        yaml,
    )
    .await
    .expect("invalid shard should write");
    fs::write(
        root.join(KNOWLEDGE_MAP_RELATIVE_PATH),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let error = service
        .route(&context, "software-model".to_owned())
        .await
        .expect_err("route must apply domain source validation");
    assert!(matches!(error, KnowledgeMapServiceError::Domain(_)));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn route_rejects_source_ids_duplicated_across_topic_shards() {
    let root = temp_root("duplicate-cross-shard-source");
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
    let mut manifest = parse_manifest(
        &fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    let topic_ref = manifest
        .topics
        .iter_mut()
        .find(|topic| topic.id == "architecture")
        .expect("architecture topic should exist");
    let mut shard: KnowledgeMapTopicShard = serde_norway::from_str(
        &fs::read_to_string(root.join(AGENT_CONTRACT_DIR_NAME).join(&topic_ref.r#ref))
            .await
            .expect("architecture shard should read"),
    )
    .expect("architecture shard should parse");
    shard.sources[0].id = "cargo".to_owned();
    shard
        .route
        .as_mut()
        .expect("architecture route should exist")
        .source_order[0] = "cargo".to_owned();
    let yaml = serialize_yaml(&shard).expect("duplicate shard should serialize");
    topic_ref.source_ids = vec!["cargo".to_owned()];
    topic_ref.digest = content_digest(yaml.as_bytes());
    topic_ref.r#ref = format!(
        "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{}.yaml",
        stable_id(&topic_ref.id),
        topic_ref.digest
    );
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(&topic_ref.r#ref),
        yaml,
    )
    .await
    .expect("duplicate shard should write");
    fs::write(
        root.join(KNOWLEDGE_MAP_RELATIVE_PATH),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let error = service
        .route(&context, "build".to_owned())
        .await
        .expect_err("root summary must reject cross-shard duplicate ids");
    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn route_rejects_blank_recent_history_fields() {
    let root = temp_root("blank-recent-history");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    let mut manifest = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    manifest.history.recent[0].action.clear();
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let error = service
        .route(&context, "software-model".to_owned())
        .await
        .expect_err("progressive route must validate recent entry contents");
    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn mutations_reject_case_colliding_topics_before_publication() {
    let root = temp_root("case-collision");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    for (id, topic) in [("lower", "build"), ("upper", "Build")] {
        let result = service
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
            .await;
        if id == "lower" {
            result.expect("first spelling should publish");
        } else {
            assert!(matches!(result, Err(KnowledgeMapServiceError::Domain(_))));
        }
    }
    let shown = service
        .show(&context, None)
        .await
        .expect("published map should remain readable");
    assert!(!shown.map.topics.iter().any(|topic| topic.id == "Build"));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn successful_publication_cleans_superseded_topic_shards() {
    let root = temp_root("shard-cleanup");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
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
        .expect("source should add");
    for index in 0..4 {
        service
            .update_source(
                &context,
                KnowledgeMapChange {
                    id: "build".to_owned(),
                    topic: None,
                    kind: None,
                    uri: None,
                    source_scope: None,
                    description: Some(format!("revision {index}")),
                },
            )
            .await
            .expect("source should update");
    }
    let manifest = parse_manifest(
        &fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    let topics = root
        .join(AGENT_CONTRACT_DIR_NAME)
        .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME);
    for name in ["README.md", ".gitkeep", "manual.yaml"] {
        fs::write(topics.join(name), "user-managed")
            .await
            .expect("user-managed file should write");
    }
    fs::remove_file(service.backup_path())
        .await
        .expect("expired recovery manifest should remove");
    cleanup_superseded_topic_shards(&root, &service.backup_path(), &manifest, Duration::ZERO).await;
    let mut entries = fs::read_dir(
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME),
    )
    .await
    .expect("topic directory should read");
    let mut shard_count = 0;
    while entries
        .next_entry()
        .await
        .expect("topic entry should read")
        .is_some()
    {
        shard_count += 1;
    }
    assert_eq!(shard_count, manifest.topics.len() + 3);
    for name in ["README.md", ".gitkeep", "manual.yaml"] {
        assert!(
            fs::try_exists(topics.join(name))
                .await
                .expect("user-managed file check should work"),
            "cleanup must preserve {name}"
        );
    }
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn shard_grace_starts_when_the_shard_becomes_unreferenced() {
    let root = temp_root("shard-retirement-time");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
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
        .expect("source should add");
    let original = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse")
    .topics
    .into_iter()
    .find(|topic| topic.id == "build")
    .expect("build topic should exist");
    let shard = root.join(AGENT_CONTRACT_DIR_NAME).join(original.r#ref);
    sleep(Duration::from_millis(20)).await;
    for description in ["revision one", "revision two"] {
        service
            .update_source(
                &context,
                KnowledgeMapChange {
                    id: "build".to_owned(),
                    topic: None,
                    kind: None,
                    uri: None,
                    source_scope: None,
                    description: Some(description.to_owned()),
                },
            )
            .await
            .expect("source should update");
    }
    let mut marker_name = shard.file_name().expect("shard name").to_os_string();
    marker_name.push(".retired");
    let marker = shard.with_file_name(marker_name);
    let shard_age = fs::metadata(&shard)
        .await
        .expect("shard metadata")
        .modified()
        .expect("shard modified time")
        .elapsed()
        .expect("shard age");
    let marker_age = fs::metadata(&marker)
        .await
        .expect("retirement marker metadata")
        .modified()
        .expect("marker modified time")
        .elapsed()
        .expect("marker age");
    assert!(shard_age > marker_age);
    let grace = marker_age + (shard_age - marker_age) / 2;
    let manifest = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("current manifest should read"),
    )
    .expect("current manifest should parse");
    cleanup_superseded_topic_shards(&root, &service.backup_path(), &manifest, grace).await;
    assert!(
        fs::try_exists(&shard).await.expect("shard check"),
        "old artifact age must not bypass its fresh retirement marker"
    );
    cleanup_superseded_topic_shards(&root, &service.backup_path(), &manifest, Duration::ZERO).await;
    assert!(!fs::try_exists(&shard).await.expect("shard check"));
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
    let mut archive_entries = fs::read_dir(
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
    )
    .await
    .expect("history directory should read");
    let mut archive_count = 0;
    while archive_entries
        .next_entry()
        .await
        .expect("archive entry should read")
        .is_some()
    {
        archive_count += 1;
    }
    assert_eq!(archive_count, 2);
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(archive_ref.r#ref),
        "tampered",
    )
    .await
    .expect("archive should tamper");
    assert!(!service.validate(&context).await.expect("validate").valid);
    let mutation = service
        .add_source(
            &context,
            KnowledgeMapSourceAddRequest {
                id: "after-tamper".to_owned(),
                topic: "build".to_owned(),
                kind: KnowledgeMapSourceKind::Doc,
                uri: "docs/tamper.md".to_owned(),
                source_scope: Some("repo".to_owned()),
                description: None,
            },
        )
        .await;
    assert!(mutation.is_err(), "mutation must verify archived history");
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

#[cfg(unix)]
#[tokio::test]
async fn rejects_a_topic_directory_symlink_to_the_contract_root() {
    use std::os::unix::fs::symlink;

    let root = temp_root("unsafe-internal-topic-symlink");
    let contract = root.join(AGENT_CONTRACT_DIR_NAME);
    fs::create_dir_all(&contract)
        .await
        .expect("contract directory should create");
    symlink(&contract, contract.join(KNOWLEDGE_MAP_TOPICS_DIR_NAME))
        .expect("topic directory symlink should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    let error = service
        .init(&context)
        .await
        .expect_err("owned topic directory must not be a symlink");
    assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
    assert!(
        !fs::try_exists(service.map_path())
            .await
            .expect("root check should work")
    );
    let _ = fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_an_existing_artifact_symlink_that_escapes_the_repository() {
    use std::os::unix::fs::symlink;

    let root = temp_root("unsafe-leaf-symlink");
    let outside = temp_root("unsafe-leaf-target");
    let map = KnowledgeMap::initial("fixture".to_owned());
    let topic = map.topics[0].clone();
    let shard = KnowledgeMapTopicShard {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        sources: map.sources.clone(),
        route: map.routes.first().cloned(),
        topic: topic.clone(),
    };
    let yaml = serialize_yaml(&shard).expect("shard should serialize");
    let digest = content_digest(yaml.as_bytes());
    let relative = format!(
        "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{digest}.yaml",
        stable_id(&topic.id)
    );
    fs::create_dir_all(
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_TOPICS_DIR_NAME),
    )
    .await
    .expect("topic directory should create");
    fs::create_dir_all(&outside)
        .await
        .expect("outside directory should create");
    let outside_file = outside.join("shard.yaml");
    fs::write(&outside_file, yaml)
        .await
        .expect("outside artifact should write");
    symlink(
        outside_file,
        root.join(AGENT_CONTRACT_DIR_NAME).join(relative),
    )
    .expect("artifact symlink should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    let error = service
        .init(&context)
        .await
        .expect_err("existing artifact symlink must be rejected");
    assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
    let _ = fs::remove_dir_all(root).await;
    let _ = fs::remove_dir_all(outside).await;
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_root_and_recovery_manifest_leaf_symlinks() {
    use std::os::unix::fs::symlink;

    for (label, use_backup) in [("root", false), ("backup", true)] {
        let root = temp_root(&format!("unsafe-{label}-leaf-symlink"));
        let outside = temp_root(&format!("unsafe-{label}-leaf-target"));
        fs::create_dir_all(root.join(AGENT_CONTRACT_DIR_NAME))
            .await
            .expect("contract directory should create");
        fs::create_dir_all(&outside)
            .await
            .expect("outside directory should create");
        let mut map = KnowledgeMap::initial("fixture".to_owned());
        map.schema_version = 1;
        let outside_file = outside.join("knowledge-map.yaml");
        fs::write(
            &outside_file,
            serialize_yaml(&map).expect("legacy map should serialize"),
        )
        .await
        .expect("outside map should write");
        let service = KnowledgeMapService::new(root.clone());
        let target = if use_backup {
            service.backup_path()
        } else {
            service.map_path()
        };
        symlink(outside_file, target).expect("root leaf symlink should create");
        let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

        let error = service
            .show(&context, None)
            .await
            .expect_err("root leaf symlink must be rejected");
        assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
        let _ = fs::remove_dir_all(root).await;
        let _ = fs::remove_dir_all(outside).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_a_symlinked_contract_directory_on_legacy_reads() {
    use std::os::unix::fs::symlink;

    let root = temp_root("unsafe-contract-directory-symlink");
    let outside = temp_root("unsafe-contract-directory-target");
    fs::create_dir_all(&root).await.expect("root should create");
    fs::create_dir_all(&outside)
        .await
        .expect("outside directory should create");
    let mut map = KnowledgeMap::initial("fixture".to_owned());
    map.schema_version = 1;
    fs::write(
        outside.join(KNOWLEDGE_MAP_FILE_NAME),
        serialize_yaml(&map).expect("legacy map should serialize"),
    )
    .await
    .expect("outside map should write");
    symlink(&outside, root.join(AGENT_CONTRACT_DIR_NAME))
        .expect("contract directory symlink should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    let error = service
        .show(&context, None)
        .await
        .expect_err("legacy read must reject a symlinked contract directory");
    assert!(matches!(error, KnowledgeMapServiceError::UnsafePath(_)));
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
        fs::try_exists(service.backup_path())
            .await
            .expect("backup check"),
        "successful publication retains the recovered root for readers"
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
