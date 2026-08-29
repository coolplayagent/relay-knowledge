use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::{
    api::{InterfaceKind, RequestContext},
    domain::KnowledgeMap,
};

#[tokio::test]
async fn legacy_map_migrates_to_visible_v3_and_rolls_back() {
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
    fs::write(
        root.join(LEGACY_KNOWLEDGE_MAP_RELATIVE_PATH),
        serde_norway::to_string(&legacy).expect("legacy map should serialize"),
    )
    .await
    .expect("legacy map should write");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);

    service
        .migrate_to_v3(&context)
        .await
        .expect("migration should work");
    let visible = fs::read_to_string(root.join(KNOWLEDGE_MAP_RELATIVE_PATH))
        .await
        .expect("visible root should exist");
    assert!(visible.contains("schema_version: 3"));
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

    let _ = fs::remove_dir_all(root).await;
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
        .replacen("schema_version: 3", "schema_version: 2", 1);
    fs::write(legacy_root, manifest)
        .await
        .expect("v2 root should write");

    service
        .validate_map_contract()
        .await
        .expect("v2 refs should resolve beside the legacy root");
    let _ = fs::remove_dir_all(root).await;
}

fn temp_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("relay-knowledge-{name}-{nonce}"))
}
