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
