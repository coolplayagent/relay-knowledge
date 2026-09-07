use super::super::migration::rewrite_contract_schema_for_test;
use super::*;
use crate::project::LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH;

#[tokio::test]
async fn legacy_history_normalizes_the_business_glossary_uri_before_validation() {
    let root = temp_root("legacy-history-glossary-uri");
    fs::create_dir_all(root.join(LEGACY_AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("legacy contract should create");
    let mut map = KnowledgeMap::initial("unix:1".to_owned());
    map.sources
        .iter_mut()
        .find(|source| source.id == "repository-business-glossary")
        .expect("reserved glossary source should exist")
        .uri = LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH.to_owned();
    fs::write(
        root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH),
        serde_norway::to_string(&map).expect("legacy map should serialize"),
    )
    .await
    .expect("legacy map should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);

    let history = service
        .history(&context, Some(1), 1)
        .await
        .expect("legacy history should normalize the glossary URI");

    assert_eq!(history.entries[0].version, 1);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn legacy_manifest_show_reports_history_outside_its_recent_window() {
    let root = temp_root("legacy-manifest-show-history");
    fs::create_dir_all(root.join(AGENT_CONTRACT_DIR_NAME))
        .await
        .expect("contract directory should create");
    let digest = "a".repeat(64);
    let manifest = KnowledgeMapManifest {
        schema_version: DIRECTORY_ARTIFACT_SCHEMA_VERSION,
        artifact_kind: Some("map".to_owned()),
        map_type: Some(RepositoryMapType::Knowledge),
        map_version: 2,
        updated_at: "unix:2".to_owned(),
        directories: baseline_directories(RepositoryMapType::Knowledge),
        topics: Vec::new(),
        history: KnowledgeMapHistoryManifest {
            archived_through: 1,
            omitted_through: 0,
            archive: Some(KnowledgeMapArchiveRef {
                r#ref: format!("history/{:020}-{:020}-{digest}.yaml", 1, 1),
                digest,
            }),
            index: None,
            recent: vec![crate::domain::KnowledgeMapHistoryEntry {
                version: 2,
                action: "fixture".to_owned(),
                actor: "test".to_owned(),
                summary: "Recent legacy entry".to_owned(),
            }],
        },
    };
    let service = KnowledgeMapService::new(root.clone());
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let shown = service
        .show(
            &RequestContext::for_interface(crate::api::InterfaceKind::Cli),
            None,
        )
        .await
        .expect("legacy manifest should remain readable");
    assert_eq!(
        shown.map.artifact_schema_version,
        DIRECTORY_ARTIFACT_SCHEMA_VERSION
    );
    assert_eq!(shown.map.history.omitted_through, 1);
    assert!(!shown.map.history.complete);
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn current_manifest_rejects_a_legacy_version_topic_shard() {
    let root = temp_root("mixed-manifest-shard-schema");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("map should initialize");
    let mut manifest = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    let topic_ref = manifest
        .topics
        .first_mut()
        .expect("fixture topic should exist");
    let mut shard: KnowledgeMapTopicShard = serde_norway::from_str(
        &fs::read_to_string(root.join(AGENT_CONTRACT_DIR_NAME).join(&topic_ref.r#ref))
            .await
            .expect("topic shard should read"),
    )
    .expect("topic shard should parse");
    shard.schema_version = DIRECTORY_ARTIFACT_SCHEMA_VERSION;
    let shard_yaml = serialize_yaml(&shard).expect("legacy-version shard should serialize");
    topic_ref.digest = content_digest(shard_yaml.as_bytes());
    topic_ref.r#ref = format!(
        "{KNOWLEDGE_MAP_TOPICS_DIR_NAME}/topic-{}-{}.yaml",
        stable_id(&topic_ref.id),
        topic_ref.digest
    );
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(&topic_ref.r#ref),
        shard_yaml,
    )
    .await
    .expect("legacy-version shard should write");
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let error = service
        .show(&context, None)
        .await
        .expect_err("show must reject a mixed-version root and shard");
    assert!(error.to_string().contains("schema does not match"));
    let validation = service
        .validate(&context)
        .await
        .expect("validation should return diagnostics");
    assert!(!validation.valid);
    assert!(
        validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("schema does not match"))
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn history_pages_reject_digest_valid_noncontiguous_archive_entries() {
    let root = temp_root("invalid-history-page");
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(crate::api::InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    rewrite_contract_schema_for_test(
        &root,
        AGENT_CONTRACT_DIR_NAME,
        &service.map_path(),
        DIRECTORY_ARTIFACT_SCHEMA_VERSION,
        None,
    )
    .await
    .expect("legacy root and shards should downgrade together");
    let manifest_text = fs::read_to_string(service.map_path())
        .await
        .expect("manifest should read");
    let mut manifest = parse_manifest(&manifest_text).expect("manifest should parse");
    let mut entries = (1..=RECENT_HISTORY_LIMIT as u64)
        .map(|version| crate::domain::KnowledgeMapHistoryEntry {
            version,
            action: "fixture".to_owned(),
            actor: "test".to_owned(),
            summary: format!("History entry {version}"),
        })
        .collect::<Vec<_>>();
    entries[5].version = 5;
    let archive = KnowledgeMapHistoryArchive {
        schema_version: DIRECTORY_ARTIFACT_SCHEMA_VERSION,
        from_version: 1,
        through_version: RECENT_HISTORY_LIMIT as u64,
        previous: None,
        entries,
    };
    let archive_yaml = serialize_yaml(&archive).expect("archive should serialize");
    let digest = content_digest(archive_yaml.as_bytes());
    let relative = format!(
        "{KNOWLEDGE_MAP_HISTORY_DIR_NAME}/{:020}-{:020}-{digest}.yaml",
        archive.from_version, archive.through_version
    );
    fs::create_dir_all(
        root.join(AGENT_CONTRACT_DIR_NAME)
            .join(KNOWLEDGE_MAP_HISTORY_DIR_NAME),
    )
    .await
    .expect("history directory should create");
    fs::write(
        root.join(AGENT_CONTRACT_DIR_NAME).join(&relative),
        archive_yaml,
    )
    .await
    .expect("archive should write");
    manifest.map_version = RECENT_HISTORY_LIMIT as u64 + 1;
    manifest.history.archived_through = RECENT_HISTORY_LIMIT as u64;
    manifest.history.omitted_through = 0;
    let archive_ref = KnowledgeMapArchiveRef {
        r#ref: relative,
        digest,
    };
    manifest.history.index = Some(
        service
            .append_history_index(None, archive_ref.clone(), &archive)
            .await
            .expect("index should publish"),
    );
    manifest.history.archive = Some(archive_ref);
    manifest.history.recent[0].version = manifest.map_version;
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("manifest should serialize"),
    )
    .await
    .expect("manifest should write");

    let error = service
        .history(&context, Some(1), RECENT_HISTORY_LIMIT)
        .await
        .expect_err("noncontiguous archive entries must fail");
    assert!(error.to_string().contains("not contiguous"));
    let _ = fs::remove_dir_all(root).await;
}
