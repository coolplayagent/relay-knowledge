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
        .history(&context, 1, 1)
        .await
        .expect("legacy history should normalize the glossary URI");

    assert_eq!(history.entries[0].version, 1);
    let _ = fs::remove_dir_all(root).await;
}
