use super::*;

use std::time::{SystemTime, UNIX_EPOCH};

use tokio::fs;

use crate::{
    api::{InterfaceKind, RequestContext},
    domain::RepositoryMapType,
};

#[tokio::test]
async fn route_rejects_a_digest_valid_manifest_with_foreign_map_identity() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-map-foreign-identity-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos()
    ));
    fs::create_dir_all(&root).await.expect("root should create");
    let service = KnowledgeMapService::new(root.clone());
    let context = RequestContext::for_interface(InterfaceKind::Cli);
    service.init(&context).await.expect("init should work");
    let mut manifest = parse_manifest(
        &fs::read_to_string(service.map_path())
            .await
            .expect("manifest should read"),
    )
    .expect("manifest should parse");
    manifest.map_type = Some(RepositoryMapType::Codespec);
    manifest.directories = contracts::baseline_directories(RepositoryMapType::Codespec);
    fs::write(
        service.map_path(),
        serialize_yaml(&manifest).expect("foreign manifest should serialize"),
    )
    .await
    .expect("foreign manifest should write");

    let error = service
        .route(&context, "software-model".to_owned())
        .await
        .expect_err("route must reject a map_type that disagrees with its path");

    assert!(matches!(error, KnowledgeMapServiceError::Integrity(_)));
    assert!(error.to_string().contains("map_type 'codespec'"));
    assert!(error.to_string().contains(KNOWLEDGE_MAP_RELATIVE_PATH));
    let _ = fs::remove_dir_all(root).await;
}
