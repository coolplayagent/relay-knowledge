//! Path-confinement tests split from the map workflow suite.

#[cfg(unix)]
use super::*;

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
