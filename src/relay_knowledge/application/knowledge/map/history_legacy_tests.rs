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
